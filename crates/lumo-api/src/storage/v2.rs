use std::str::FromStr;

use base64::Engine as _;
use lumo_core::{
    domain::{AppSnapshot, RuntimeState},
    security::{ReplayGuard, SealedPayload, SessionCipher},
    LumoError, LumoResult, LumoService,
};
use lumo_protocol::{
    ControlledOperation, ControlledOperationRequest, ControlledOperationResponse,
    DeviceCredentialResponse, DeviceRole, DeviceSummary, RemoteStateRecord,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{de::DeserializeOwned, Serialize};
use zeroize::Zeroizing;

use crate::{auth::DeviceAuthAttempt, config::ApiLimits, crypto::MasterKey};

use super::{storage_error, ApiStore};

const SCHEMA_VERSION: i64 = 5;
const PIN_MAX_ATTEMPTS: i64 = 5;
const PIN_LOCK_MS: i64 = 5 * 60 * 1_000;
const MAX_NONCES_PER_DEVICE: i64 = 256;
const NONCE_TTL_MS: i64 = 5 * 60 * 1_000;
const LAST_SEEN_WRITE_INTERVAL_MS: i64 = 60 * 1_000;
const UNINITIALIZED_GROUP_TTL_MS: i64 = 24 * 60 * 60 * 1_000;
const REVOKED_DEVICE_TTL_MS: i64 = 24 * 60 * 60 * 1_000;
const IDEMPOTENCY_TTL_MS: i64 = 24 * 60 * 60 * 1_000;
const MEMBER_ENVELOPE_TTL_MS: i64 = 5 * 60 * 1_000;
const MASTER_KEY_CHECK_META: &str = "master_key_check_v1";

#[derive(Debug, Clone)]
pub struct NewDevice {
    pub id: String,
    pub name: String,
    pub role: DeviceRole,
    pub token_hash: Vec<u8>,
    pub member_key_nonce: Option<Vec<u8>>,
    pub member_key_ciphertext: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct NewGroup {
    pub id: String,
    pub pin_hash: String,
    pub state_key_nonce: Vec<u8>,
    pub state_key_ciphertext: Vec<u8>,
    pub controller: NewDevice,
}

pub struct NewInvitation {
    pub id: String,
    pub group_id: String,
    pub controller_id: String,
    pub pin: Zeroizing<String>,
    pub token_hash: Vec<u8>,
    pub role: DeviceRole,
    pub created_at_ms: i64,
}

pub struct ConsumeInvitation {
    pub request_id: String,
    pub request_digest: Vec<u8>,
    pub invitation_id: String,
    pub token: Zeroizing<String>,
    pub pin: Zeroizing<String>,
    pub device: NewDevice,
    pub consumed_at_ms: i64,
    pub device_token: Zeroizing<String>,
    pub member_key: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedDevice {
    pub group_id: String,
    pub device_id: String,
    pub role: DeviceRole,
}

#[derive(Debug, Clone)]
pub struct ConsumedInvitation {
    pub credential: DeviceCredentialResponse,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Idempotent<T> {
    Fresh(T),
    Replay(T),
    Conflict,
}

#[derive(Debug, Clone)]
pub struct MemberOperationResult {
    pub member_key: [u8; 32],
    pub response: ControlledOperationResponse,
}

#[derive(Debug, Clone)]
pub struct MemberSnapshotResult {
    pub member_key: [u8; 32],
    pub snapshot: AppSnapshot,
}

type StoredInvitationRow = (
    String,
    Vec<u8>,
    i64,
    Option<i64>,
    i64,
    Option<i64>,
    String,
    String,
);
type WrappedMemberKeyRow = (Option<Vec<u8>>, Option<Vec<u8>>);
type StoredReplayRow = (Vec<u8>, Vec<u8>, Vec<u8>, i64);

pub(super) fn migrate(connection: &Connection) -> LumoResult<()> {
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(storage_error)?;
    if version > SCHEMA_VERSION {
        return Err(LumoError::Storage(format!(
            "database schema version {version} is newer than supported version {SCHEMA_VERSION}"
        )));
    }
    connection
        .execute_batch(
            "BEGIN IMMEDIATE;
             CREATE TABLE IF NOT EXISTS groups_v2 (
                id TEXT PRIMARY KEY,
                pin_hash TEXT NOT NULL,
                state_key_nonce BLOB NOT NULL,
                state_key_ciphertext BLOB NOT NULL,
                created_at_ms INTEGER NOT NULL,
                initialized_at_ms INTEGER,
                pin_failed_attempts INTEGER NOT NULL DEFAULT 0,
                pin_locked_until_ms INTEGER
             );
             CREATE TABLE IF NOT EXISTS group_state_v2 (
                group_id TEXT PRIMARY KEY REFERENCES groups_v2(id) ON DELETE CASCADE,
                revision INTEGER NOT NULL,
                payload BLOB NOT NULL
             );
             CREATE TABLE IF NOT EXISTS devices_v2 (
                id TEXT PRIMARY KEY,
                group_id TEXT NOT NULL REFERENCES groups_v2(id) ON DELETE CASCADE,
                name TEXT NOT NULL,
                role TEXT NOT NULL CHECK(role IN ('controller', 'controlled')),
                token_hash BLOB NOT NULL,
                member_key_nonce BLOB,
                member_key_ciphertext BLOB,
                created_at_ms INTEGER NOT NULL,
                last_seen_at_ms INTEGER NOT NULL,
                revoked_at_ms INTEGER
             );
             CREATE UNIQUE INDEX IF NOT EXISTS devices_v2_token_hash
                ON devices_v2(token_hash);
             DROP INDEX IF EXISTS devices_v2_one_controller;
             CREATE UNIQUE INDEX IF NOT EXISTS devices_v2_one_controlled
                ON devices_v2(group_id)
                WHERE role = 'controlled' AND revoked_at_ms IS NULL;
             CREATE INDEX IF NOT EXISTS devices_v2_group
                ON devices_v2(group_id, revoked_at_ms);
             CREATE TABLE IF NOT EXISTS invitations_v2 (
                id TEXT PRIMARY KEY,
                group_id TEXT NOT NULL REFERENCES groups_v2(id) ON DELETE CASCADE,
                token_hash BLOB NOT NULL,
                role TEXT NOT NULL DEFAULT 'controlled' CHECK(role IN ('controller', 'controlled')),
                created_by_device_id TEXT NOT NULL REFERENCES devices_v2(id),
                created_at_ms INTEGER NOT NULL,
                expires_at_ms INTEGER NOT NULL,
                used_at_ms INTEGER,
                failed_attempts INTEGER NOT NULL DEFAULT 0,
                locked_until_ms INTEGER
             );
             CREATE UNIQUE INDEX IF NOT EXISTS invitations_v2_token_hash
                ON invitations_v2(token_hash);
             CREATE INDEX IF NOT EXISTS invitations_v2_group_active
                ON invitations_v2(group_id, expires_at_ms, used_at_ms);
             CREATE TABLE IF NOT EXISTS device_nonces_v2 (
                device_id TEXT NOT NULL REFERENCES devices_v2(id) ON DELETE CASCADE,
                nonce TEXT NOT NULL,
                accepted_at_ms INTEGER NOT NULL,
                PRIMARY KEY(device_id, nonce)
             );
             CREATE INDEX IF NOT EXISTS device_nonces_v2_expiry
                ON device_nonces_v2(accepted_at_ms);
             CREATE TABLE IF NOT EXISTS bootstrap_limits_v2 (
                scope_key TEXT PRIMARY KEY,
                window_started_ms INTEGER NOT NULL,
                attempts INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS bootstrap_requests_v2 (
                request_id TEXT PRIMARY KEY,
                request_digest BLOB NOT NULL,
                created_at_ms INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS idempotency_v2 (
                kind TEXT NOT NULL,
                request_id TEXT NOT NULL,
                request_digest BLOB NOT NULL,
                response_nonce BLOB NOT NULL,
                response_ciphertext BLOB NOT NULL,
                created_at_ms INTEGER NOT NULL,
                PRIMARY KEY(kind, request_id)
             );
             CREATE INDEX IF NOT EXISTS idempotency_v2_expiry
                ON idempotency_v2(created_at_ms);
             CREATE TABLE IF NOT EXISTS member_operations_v2 (
                device_id TEXT NOT NULL REFERENCES devices_v2(id) ON DELETE CASCADE,
                operation_id TEXT NOT NULL,
                request_digest BLOB NOT NULL,
                response_nonce BLOB NOT NULL,
                response_ciphertext BLOB NOT NULL,
                created_at_ms INTEGER NOT NULL,
                PRIMARY KEY(device_id, operation_id)
             );
             CREATE INDEX IF NOT EXISTS member_operations_v2_expiry
                ON member_operations_v2(created_at_ms);
             CREATE TABLE IF NOT EXISTS device_pin_guards_v2 (
                device_id TEXT PRIMARY KEY REFERENCES devices_v2(id) ON DELETE CASCADE,
                failed_attempts INTEGER NOT NULL DEFAULT 0,
                locked_until_ms INTEGER
             );
             CREATE TABLE IF NOT EXISTS server_meta_v2 (
                key TEXT PRIMARY KEY,
                value BLOB NOT NULL
             );
             COMMIT;",
        )
        .map_err(storage_error)?;
    add_column_if_missing(connection, "devices_v2", "member_key_nonce", "BLOB")?;
    add_column_if_missing(connection, "devices_v2", "member_key_ciphertext", "BLOB")?;
    add_column_if_missing(
        connection,
        "invitations_v2",
        "role",
        "TEXT NOT NULL DEFAULT 'controlled' CHECK(role IN ('controller', 'controlled'))",
    )?;
    connection
        .pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(storage_error)
}

pub(super) fn verify_or_initialize_master(
    connection: &Connection,
    master: &MasterKey,
) -> LumoResult<()> {
    let stored: Option<Vec<u8>> = connection
        .query_row(
            "SELECT value FROM server_meta_v2 WHERE key = ?1",
            params![MASTER_KEY_CHECK_META],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    if let Some(stored) = stored {
        return if master.verify_database_key_check(&stored) {
            Ok(())
        } else {
            Err(master_key_mismatch())
        };
    }

    // A pre-v3 database has no verifier. Existing wrapped state keys provide a fail-closed
    // migration proof before the verifier is initialized with the configured master key.
    let wrapped_groups = {
        let mut statement = connection
            .prepare("SELECT id, state_key_nonce, state_key_ciphertext FROM groups_v2")
            .map_err(storage_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                ))
            })
            .map_err(storage_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage_error)?;
        rows
    };
    for (group_id, nonce, ciphertext) in wrapped_groups {
        master
            .unwrap_state_key(&group_id, &nonce, &ciphertext)
            .map_err(|_| master_key_mismatch())?;
    }
    connection
        .execute(
            "INSERT INTO server_meta_v2(key, value) VALUES(?1, ?2)",
            params![MASTER_KEY_CHECK_META, master.database_key_check()],
        )
        .map_err(storage_error)?;
    Ok(())
}

fn master_key_mismatch() -> LumoError {
    LumoError::Configuration("configured server master key does not match this database".to_owned())
}

impl ApiStore {
    pub(crate) fn load_create_group_replay_v2(
        &self,
        master: &MasterKey,
        request_id: &str,
        request_digest: &[u8],
        now_ms: i64,
    ) -> LumoResult<Option<Idempotent<DeviceCredentialResponse>>> {
        let connection = self.lock()?;
        load_idempotent(
            &connection,
            master,
            "create_group",
            request_id,
            request_digest,
            now_ms.saturating_sub(IDEMPOTENCY_TTL_MS),
        )
    }

    pub fn reserve_group_bootstrap_v2(
        &self,
        bootstrap_key: &str,
        request_id: &str,
        request_digest: &[u8],
        now_ms: i64,
        limits: &ApiLimits,
    ) -> LumoResult<Idempotent<()>> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        cleanup(&transaction, now_ms, limits.bootstrap_window_ms)?;
        let reserved_digest: Option<Vec<u8>> = transaction
            .query_row(
                "SELECT request_digest FROM bootstrap_requests_v2 WHERE request_id = ?1",
                params![request_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        if let Some(reserved_digest) = reserved_digest {
            transaction.commit().map_err(storage_error)?;
            return Ok(if reserved_digest == request_digest {
                Idempotent::Replay(())
            } else {
                Idempotent::Conflict
            });
        }
        consume_bootstrap_limit(
            &transaction,
            "global",
            limits.bootstrap_global,
            limits.bootstrap_window_ms,
            now_ms,
        )?;
        consume_bootstrap_limit(
            &transaction,
            bootstrap_key,
            limits.bootstrap_per_ip,
            limits.bootstrap_window_ms,
            now_ms,
        )?;
        let count: u32 = transaction
            .query_row("SELECT COUNT(*) FROM groups_v2", [], |row| row.get(0))
            .map_err(storage_error)?;
        if count >= limits.max_groups {
            return Err(LumoError::RateLimited);
        }
        transaction
            .execute(
                "INSERT INTO bootstrap_requests_v2(request_id, request_digest, created_at_ms)
                 VALUES(?1, ?2, ?3)",
                params![request_id, request_digest, now_ms],
            )
            .map_err(storage_error)?;
        transaction.commit().map_err(storage_error)?;
        Ok(Idempotent::Fresh(()))
    }

    pub fn create_group_v2(
        &self,
        group: &NewGroup,
        now_ms: i64,
        limits: &ApiLimits,
    ) -> LumoResult<()> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        cleanup(&transaction, now_ms, limits.bootstrap_window_ms)?;
        let count: u32 = transaction
            .query_row("SELECT COUNT(*) FROM groups_v2", [], |row| row.get(0))
            .map_err(storage_error)?;
        if count >= limits.max_groups {
            return Err(LumoError::RateLimited);
        }
        transaction
            .execute(
                "INSERT INTO groups_v2(
                    id, pin_hash, state_key_nonce, state_key_ciphertext, created_at_ms
                 ) VALUES(?1, ?2, ?3, ?4, ?5)",
                params![
                    group.id,
                    group.pin_hash,
                    group.state_key_nonce,
                    group.state_key_ciphertext,
                    now_ms
                ],
            )
            .map_err(storage_error)?;
        insert_device(&transaction, &group.id, &group.controller, now_ms)?;
        transaction.commit().map_err(storage_error)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_group_idempotent_v2(
        &self,
        master: &MasterKey,
        request_id: &str,
        request_digest: &[u8],
        group: &NewGroup,
        credential: &DeviceCredentialResponse,
        now_ms: i64,
        limits: &ApiLimits,
    ) -> LumoResult<Idempotent<DeviceCredentialResponse>> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        cleanup(&transaction, now_ms, limits.bootstrap_window_ms)?;
        if let Some(replay) = load_idempotent::<DeviceCredentialResponse>(
            &transaction,
            master,
            "create_group",
            request_id,
            request_digest,
            now_ms.saturating_sub(IDEMPOTENCY_TTL_MS),
        )? {
            return match replay {
                Idempotent::Replay(value) => {
                    transaction.commit().map_err(storage_error)?;
                    Ok(Idempotent::Replay(value))
                }
                Idempotent::Conflict => {
                    transaction.commit().map_err(storage_error)?;
                    Ok(Idempotent::Conflict)
                }
                Idempotent::Fresh(_) => unreachable!("stored records are replays"),
            };
        }
        let reserved_digest: Option<Vec<u8>> = transaction
            .query_row(
                "SELECT request_digest FROM bootstrap_requests_v2 WHERE request_id = ?1",
                params![request_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        match reserved_digest {
            Some(digest) if digest == request_digest => {}
            Some(_) => return Ok(Idempotent::Conflict),
            None => {
                return Err(LumoError::Storage(
                    "group bootstrap was not reserved before PIN hashing".to_owned(),
                ))
            }
        }
        let count: u32 = transaction
            .query_row("SELECT COUNT(*) FROM groups_v2", [], |row| row.get(0))
            .map_err(storage_error)?;
        if count >= limits.max_groups {
            return Err(LumoError::RateLimited);
        }
        transaction
            .execute(
                "INSERT INTO groups_v2(
                    id, pin_hash, state_key_nonce, state_key_ciphertext, created_at_ms
                 ) VALUES(?1, ?2, ?3, ?4, ?5)",
                params![
                    group.id,
                    group.pin_hash,
                    group.state_key_nonce,
                    group.state_key_ciphertext,
                    now_ms
                ],
            )
            .map_err(storage_error)?;
        insert_device(&transaction, &group.id, &group.controller, now_ms)?;
        store_idempotent(
            &transaction,
            master,
            "create_group",
            request_id,
            request_digest,
            credential,
            now_ms,
        )?;
        transaction.commit().map_err(storage_error)?;
        Ok(Idempotent::Fresh(credential.clone()))
    }

    pub fn authenticate_device_read_v2(
        &self,
        master: &MasterKey,
        group_id: &str,
        device_id: &str,
        token: &str,
        now_ms: i64,
    ) -> LumoResult<AuthenticatedDevice> {
        let connection = self.lock()?;
        let row: Option<(String, Vec<u8>, Option<i64>)> = connection
            .query_row(
                "SELECT role, token_hash, revoked_at_ms
                 FROM devices_v2 WHERE id = ?1 AND group_id = ?2",
                params![device_id, group_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(storage_error)?;
        let (role, token_hash, revoked_at_ms) = row.ok_or(LumoError::AuthenticationFailed)?;
        if revoked_at_ms.is_some() || !master.verify_token_hash(token, &token_hash) {
            return Err(LumoError::AuthenticationFailed);
        }
        update_last_seen(&connection, device_id, now_ms)?;
        Ok(AuthenticatedDevice {
            group_id: group_id.to_owned(),
            device_id: device_id.to_owned(),
            role: DeviceRole::from_str(&role)?,
        })
    }

    pub fn authenticate_device_mutation_v2(
        &self,
        master: &MasterKey,
        group_id: &str,
        device_id: &str,
        token: &str,
        nonce: &str,
        now_ms: i64,
    ) -> LumoResult<AuthenticatedDevice> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let row: Option<(String, Vec<u8>, Option<i64>)> = transaction
            .query_row(
                "SELECT role, token_hash, revoked_at_ms
                 FROM devices_v2 WHERE id = ?1 AND group_id = ?2",
                params![device_id, group_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(storage_error)?;
        let (role, token_hash, revoked_at_ms) = row.ok_or(LumoError::AuthenticationFailed)?;
        if revoked_at_ms.is_some() || !master.verify_token_hash(token, &token_hash) {
            return Err(LumoError::AuthenticationFailed);
        }
        transaction
            .execute(
                "DELETE FROM device_nonces_v2 WHERE accepted_at_ms < ?1",
                params![now_ms.saturating_sub(NONCE_TTL_MS)],
            )
            .map_err(storage_error)?;
        let nonce_count: i64 = transaction
            .query_row(
                "SELECT COUNT(*) FROM device_nonces_v2 WHERE device_id = ?1",
                params![device_id],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if nonce_count >= MAX_NONCES_PER_DEVICE {
            return Err(LumoError::RateLimited);
        }
        let inserted = transaction
            .execute(
                "INSERT OR IGNORE INTO device_nonces_v2(device_id, nonce, accepted_at_ms)
                 VALUES(?1, ?2, ?3)",
                params![device_id, nonce, now_ms],
            )
            .map_err(storage_error)?;
        if inserted == 0 {
            return Err(LumoError::ReplayDetected);
        }
        update_last_seen(&transaction, device_id, now_ms)?;
        transaction.commit().map_err(storage_error)?;
        Ok(AuthenticatedDevice {
            group_id: group_id.to_owned(),
            device_id: device_id.to_owned(),
            role: DeviceRole::from_str(&role)?,
        })
    }

    pub fn load_state_v2(&self, group_id: &str) -> LumoResult<Option<RemoteStateRecord>> {
        let connection = self.lock()?;
        let encoded: Option<Vec<u8>> = connection
            .query_row(
                "SELECT payload FROM group_state_v2 WHERE group_id = ?1",
                params![group_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        decode_record(encoded)
    }

    pub fn compare_and_swap_v2(
        &self,
        group_id: &str,
        expected_revision: Option<u64>,
        record: &RemoteStateRecord,
        now_ms: i64,
    ) -> LumoResult<bool> {
        record.validate()?;
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        ensure_group(&transaction, group_id)?;
        let current: Option<u64> = transaction
            .query_row(
                "SELECT revision FROM group_state_v2 WHERE group_id = ?1",
                params![group_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        if current != expected_revision {
            return Ok(false);
        }
        if record.revision <= current.unwrap_or_default() {
            return Err(LumoError::InvalidInput(
                "state revision must advance monotonically".to_owned(),
            ));
        }
        let encoded = serde_json::to_vec(record)
            .map_err(|error| LumoError::Serialization(error.to_string()))?;
        transaction
            .execute(
                "INSERT INTO group_state_v2(group_id, revision, payload)
                 VALUES(?1, ?2, ?3)
                 ON CONFLICT(group_id) DO UPDATE SET
                    revision = excluded.revision,
                    payload = excluded.payload",
                params![group_id, record.revision, encoded],
            )
            .map_err(storage_error)?;
        transaction
            .execute(
                "UPDATE groups_v2 SET initialized_at_ms = COALESCE(initialized_at_ms, ?2)
                 WHERE id = ?1",
                params![group_id, now_ms],
            )
            .map_err(storage_error)?;
        transaction.commit().map_err(storage_error)?;
        Ok(true)
    }

    pub fn create_invitation_v2(
        &self,
        master: &MasterKey,
        invitation: &NewInvitation,
        limits: &ApiLimits,
    ) -> LumoResult<()> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        if let Err(error) = verify_group_pin(
            &transaction,
            master,
            &invitation.group_id,
            &invitation.controller_id,
            &invitation.pin,
            invitation.created_at_ms,
        ) {
            transaction.commit().map_err(storage_error)?;
            return Err(error);
        }
        transaction
            .execute(
                "DELETE FROM invitations_v2
                 WHERE group_id = ?1 AND (expires_at_ms < ?2 OR used_at_ms IS NOT NULL)",
                params![invitation.group_id, invitation.created_at_ms],
            )
            .map_err(storage_error)?;
        let active: u32 = transaction
            .query_row(
                "SELECT COUNT(*) FROM invitations_v2 WHERE group_id = ?1",
                params![invitation.group_id],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if active >= limits.max_active_invites_per_group {
            return Err(LumoError::RateLimited);
        }
        if invitation.role == DeviceRole::Controlled {
            let active_controlled: u32 = transaction
                .query_row(
                    "SELECT COUNT(*) FROM devices_v2
                     WHERE group_id = ?1 AND role = 'controlled' AND revoked_at_ms IS NULL",
                    params![invitation.group_id],
                    |row| row.get(0),
                )
                .map_err(storage_error)?;
            if active_controlled > 0 {
                return Err(LumoError::InvalidInput(
                    "the group already has a controlled device".to_owned(),
                ));
            }
        }
        transaction
            .execute(
                "INSERT INTO invitations_v2(
                    id, group_id, token_hash, created_by_device_id,
                    created_at_ms, expires_at_ms, role
                 ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    invitation.id,
                    invitation.group_id,
                    invitation.token_hash,
                    invitation.controller_id,
                    invitation.created_at_ms,
                    invitation
                        .created_at_ms
                        .saturating_add(limits.invite_ttl_ms),
                    invitation.role.as_str(),
                ],
            )
            .map_err(storage_error)?;
        transaction.commit().map_err(storage_error)
    }

    pub fn consume_invitation_v2(
        &self,
        master: &MasterKey,
        consumption: &ConsumeInvitation,
        limits: &ApiLimits,
    ) -> LumoResult<Idempotent<ConsumedInvitation>> {
        let invitation_id = &consumption.invitation_id;
        let token = &consumption.token;
        let pin = &consumption.pin;
        let device = &consumption.device;
        let now_ms = consumption.consumed_at_ms;
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        cleanup(&transaction, now_ms, limits.bootstrap_window_ms)?;
        if let Some(replay) = load_idempotent::<DeviceCredentialResponse>(
            &transaction,
            master,
            "consume_invitation",
            &consumption.request_id,
            &consumption.request_digest,
            now_ms.saturating_sub(IDEMPOTENCY_TTL_MS),
        )? {
            return match replay {
                Idempotent::Replay(credential) => {
                    transaction.commit().map_err(storage_error)?;
                    Ok(Idempotent::Replay(ConsumedInvitation { credential }))
                }
                Idempotent::Conflict => {
                    transaction.commit().map_err(storage_error)?;
                    Ok(Idempotent::Conflict)
                }
                Idempotent::Fresh(_) => unreachable!("stored records are replays"),
            };
        }
        let invitation: Option<StoredInvitationRow> = transaction
            .query_row(
                "SELECT i.group_id, i.token_hash, i.expires_at_ms, i.used_at_ms,
                        i.failed_attempts, i.locked_until_ms, g.pin_hash, i.role
                 FROM invitations_v2 i
                 JOIN groups_v2 g ON g.id = i.group_id
                 WHERE i.id = ?1",
                params![invitation_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                    ))
                },
            )
            .optional()
            .map_err(storage_error)?;
        let Some((
            group_id,
            token_hash,
            expires_at_ms,
            used_at_ms,
            mut failed_attempts,
            locked_until_ms,
            pin_hash,
            invited_role,
        )) = invitation
        else {
            return Err(LumoError::InvalidInvitation);
        };
        if used_at_ms.is_some() || expires_at_ms < now_ms {
            return Err(LumoError::InvalidInvitation);
        }
        if locked_until_ms.is_some_and(|until| until > now_ms) {
            return Err(LumoError::RateLimited);
        }
        if locked_until_ms.is_some() {
            failed_attempts = 0;
        }
        let credentials_match = master.verify_token_hash(token, &token_hash)
            && master.verify_group_pin(&group_id, pin, &pin_hash);
        if !credentials_match {
            failed_attempts = failed_attempts.saturating_add(1);
            let locked_until =
                (failed_attempts >= PIN_MAX_ATTEMPTS).then(|| now_ms.saturating_add(PIN_LOCK_MS));
            transaction
                .execute(
                    "UPDATE invitations_v2
                     SET failed_attempts = ?2, locked_until_ms = ?3
                     WHERE id = ?1",
                    params![invitation_id, failed_attempts, locked_until],
                )
                .map_err(storage_error)?;
            transaction.commit().map_err(storage_error)?;
            return Err(if locked_until.is_some() {
                LumoError::RateLimited
            } else {
                LumoError::InvalidInvitation
            });
        }
        transaction
            .execute(
                "DELETE FROM devices_v2
                 WHERE group_id = ?1 AND role = 'controlled' AND revoked_at_ms IS NOT NULL",
                params![group_id],
            )
            .map_err(storage_error)?;
        let devices: u32 = transaction
            .query_row(
                "SELECT COUNT(*) FROM devices_v2
                 WHERE group_id = ?1 AND revoked_at_ms IS NULL",
                params![group_id],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if devices >= limits.max_devices_per_group {
            return Err(LumoError::RateLimited);
        }
        let invited_role = DeviceRole::from_str(&invited_role)?;
        if invited_role == DeviceRole::Controlled {
            let active_controlled: u32 = transaction
                .query_row(
                    "SELECT COUNT(*) FROM devices_v2
                     WHERE group_id = ?1 AND role = 'controlled' AND revoked_at_ms IS NULL",
                    params![group_id],
                    |row| row.get(0),
                )
                .map_err(storage_error)?;
            if active_controlled > 0 {
                return Err(LumoError::InvalidInput(
                    "the group already has a controlled device".to_owned(),
                ));
            }
        }
        let mut provisioned_device = device.clone();
        provisioned_device.role = invited_role;
        if invited_role == DeviceRole::Controlled {
            let (member_key_nonce, member_key_ciphertext) =
                master.wrap_member_key(&group_id, &device.id, &consumption.member_key)?;
            provisioned_device.member_key_nonce = Some(member_key_nonce);
            provisioned_device.member_key_ciphertext = Some(member_key_ciphertext);
        } else {
            provisioned_device.member_key_nonce = None;
            provisioned_device.member_key_ciphertext = None;
        }
        insert_device(&transaction, &group_id, &provisioned_device, now_ms)?;
        let updated = transaction
            .execute(
                "UPDATE invitations_v2 SET used_at_ms = ?2
                 WHERE id = ?1 AND used_at_ms IS NULL",
                params![invitation_id, now_ms],
            )
            .map_err(storage_error)?;
        if updated != 1 {
            return Err(LumoError::InvalidInvitation);
        }
        let credential_key = if invited_role == DeviceRole::Controlled {
            consumption.member_key
        } else {
            load_canonical_key(&transaction, master, &group_id)?
        };
        let credential = DeviceCredentialResponse {
            group_id,
            device_id: device.id.clone(),
            role: invited_role,
            device_token: consumption.device_token.to_string(),
            state_key: base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(credential_key),
        };
        store_idempotent(
            &transaction,
            master,
            "consume_invitation",
            &consumption.request_id,
            &consumption.request_digest,
            &credential,
            now_ms,
        )?;
        transaction.commit().map_err(storage_error)?;
        Ok(Idempotent::Fresh(ConsumedInvitation { credential }))
    }

    pub(crate) fn load_member_snapshot_v2(
        &self,
        master: &MasterKey,
        group_id: &str,
        auth: &DeviceAuthAttempt,
        now_ms: i64,
    ) -> LumoResult<Option<MemberSnapshotResult>> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        cleanup_replays(&transaction, now_ms)?;
        authenticate_transaction(
            &transaction,
            master,
            group_id,
            auth,
            now_ms,
            false,
            Some(DeviceRole::Controlled),
        )?;
        let member_key = load_member_key(&transaction, master, group_id, &auth.device_id)?;
        let state = load_runtime_state(&transaction, master, group_id, now_ms)?;
        let result = state.map(|(state, _key, _record)| MemberSnapshotResult {
            member_key,
            snapshot: state.member_snapshot(),
        });
        transaction.commit().map_err(storage_error)?;
        Ok(result)
    }

    pub(crate) fn apply_member_operation_v2(
        &self,
        master: &MasterKey,
        group_id: &str,
        auth: &DeviceAuthAttempt,
        envelope: &SealedPayload,
        now_ms: i64,
    ) -> LumoResult<Idempotent<MemberOperationResult>> {
        validate_member_envelope(envelope, now_ms)?;
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        cleanup_replays(&transaction, now_ms)?;
        authenticate_transaction(
            &transaction,
            master,
            group_id,
            auth,
            now_ms,
            true,
            Some(DeviceRole::Controlled),
        )?;
        let member_key = load_member_key(&transaction, master, group_id, &auth.device_id)?;
        let request: ControlledOperationRequest = SessionCipher::from_key(member_key).open(
            envelope,
            now_ms,
            &mut ReplayGuard::default(),
        )?;
        uuid::Uuid::parse_str(&request.operation_id)
            .map_err(|_| LumoError::InvalidInput("operationId must be a UUID".to_owned()))?;
        let operation_bytes = serde_json::to_vec(&request.operation)
            .map_err(|error| LumoError::Serialization(error.to_string()))?;
        let request_digest = master.idempotency_digest(
            "member_operation",
            &[
                group_id.as_bytes(),
                auth.device_id.as_bytes(),
                &operation_bytes,
            ],
        );
        if let Some(replay) = load_member_operation(
            &transaction,
            master,
            &auth.device_id,
            &request.operation_id,
            &request_digest,
        )? {
            return match replay {
                Idempotent::Replay(response) => {
                    transaction.commit().map_err(storage_error)?;
                    Ok(Idempotent::Replay(MemberOperationResult {
                        member_key,
                        response,
                    }))
                }
                Idempotent::Conflict => {
                    transaction.commit().map_err(storage_error)?;
                    Ok(Idempotent::Conflict)
                }
                Idempotent::Fresh(_) => unreachable!("stored operations are replays"),
            };
        }
        let (mut runtime, canonical_key, current_record) =
            load_runtime_state(&transaction, master, group_id, now_ms)?
                .ok_or(LumoError::GroupNotInitialized)?;
        let previous_revision = runtime.revision;
        let processed = match apply_controlled_operation(&mut runtime, request.operation, now_ms) {
            Ok(processed) => processed,
            Err(error) => {
                transaction.commit().map_err(storage_error)?;
                return Err(error);
            }
        };
        let response = ControlledOperationResponse {
            snapshot: runtime.member_snapshot(),
            processed,
        };
        if runtime.revision != previous_revision {
            if runtime.revision <= current_record.revision {
                return Err(LumoError::Storage(
                    "member operation did not advance the state revision".to_owned(),
                ));
            }
            let record = RemoteStateRecord {
                revision: runtime.revision,
                envelope: SessionCipher::from_key(canonical_key).seal(
                    &runtime,
                    now_ms,
                    i64::MAX.saturating_sub(now_ms),
                )?,
            };
            persist_state(&transaction, group_id, &record, now_ms)?;
        }
        store_member_operation(
            &transaction,
            master,
            &auth.device_id,
            &request.operation_id,
            &request_digest,
            &response,
            now_ms,
        )?;
        transaction.commit().map_err(storage_error)?;
        Ok(Idempotent::Fresh(MemberOperationResult {
            member_key,
            response,
        }))
    }

    pub(crate) fn verify_pin_authorized_v2(
        &self,
        master: &MasterKey,
        group_id: &str,
        auth: &DeviceAuthAttempt,
        pin: &str,
        now_ms: i64,
    ) -> LumoResult<()> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        authenticate_transaction(&transaction, master, group_id, auth, now_ms, true, None)?;
        if let Err(error) =
            verify_group_pin(&transaction, master, group_id, &auth.device_id, pin, now_ms)
        {
            transaction.commit().map_err(storage_error)?;
            return Err(error);
        }
        transaction.commit().map_err(storage_error)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn revoke_device_authorized_v2(
        &self,
        master: &MasterKey,
        group_id: &str,
        auth: &DeviceAuthAttempt,
        target_device_id: &str,
        pin: &str,
        now_ms: i64,
    ) -> LumoResult<()> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        authenticate_transaction(
            &transaction,
            master,
            group_id,
            auth,
            now_ms,
            true,
            Some(DeviceRole::Controller),
        )?;
        if auth.device_id == target_device_id {
            return Err(LumoError::Unauthorized);
        }
        if let Err(error) =
            verify_group_pin(&transaction, master, group_id, &auth.device_id, pin, now_ms)
        {
            transaction.commit().map_err(storage_error)?;
            return Err(error);
        }
        let target_role: String = transaction
            .query_row(
                "SELECT role FROM devices_v2
                 WHERE id = ?1 AND group_id = ?2 AND revoked_at_ms IS NULL",
                params![target_device_id, group_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?
            .ok_or_else(|| LumoError::NotFound("device".to_owned()))?;
        if DeviceRole::from_str(&target_role)? == DeviceRole::Controller {
            let active_controllers: u32 = transaction
                .query_row(
                    "SELECT COUNT(*) FROM devices_v2
                     WHERE group_id = ?1 AND role = 'controller' AND revoked_at_ms IS NULL",
                    params![group_id],
                    |row| row.get(0),
                )
                .map_err(storage_error)?;
            if active_controllers <= 1 {
                return Err(LumoError::Unauthorized);
            }
        }
        let changed = transaction
            .execute(
                "UPDATE devices_v2 SET revoked_at_ms = ?3
                 WHERE id = ?1 AND group_id = ?2 AND revoked_at_ms IS NULL",
                params![target_device_id, group_id, now_ms],
            )
            .map_err(storage_error)?;
        if changed != 1 {
            return Err(LumoError::NotFound("device".to_owned()));
        }
        transaction.commit().map_err(storage_error)
    }

    pub fn list_devices_v2(&self, group_id: &str) -> LumoResult<Vec<DeviceSummary>> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare_cached(
                "SELECT id, name, role, created_at_ms, last_seen_at_ms, revoked_at_ms
                 FROM devices_v2 WHERE group_id = ?1
                 ORDER BY created_at_ms, id",
            )
            .map_err(storage_error)?;
        let rows = statement
            .query_map(params![group_id], |row| {
                let role: String = row.get(2)?;
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    role,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                ))
            })
            .map_err(storage_error)?;
        rows.map(|row| {
            let (device_id, device_name, role, created_at_ms, last_seen_at_ms, revoked_at_ms) =
                row.map_err(storage_error)?;
            Ok(DeviceSummary {
                device_id,
                device_name,
                role: DeviceRole::from_str(&role)?,
                created_at_ms,
                last_seen_at_ms,
                revoked_at_ms,
            })
        })
        .collect()
    }

    pub fn revoke_device_v2(
        &self,
        group_id: &str,
        actor_device_id: &str,
        target_device_id: &str,
        now_ms: i64,
    ) -> LumoResult<()> {
        if actor_device_id == target_device_id {
            return Err(LumoError::Unauthorized);
        }
        let connection = self.lock()?;
        let changed = connection
            .execute(
                "UPDATE devices_v2 SET revoked_at_ms = ?3
                 WHERE id = ?1 AND group_id = ?2 AND revoked_at_ms IS NULL",
                params![target_device_id, group_id, now_ms],
            )
            .map_err(storage_error)?;
        if changed == 1 {
            Ok(())
        } else {
            Err(LumoError::NotFound("device".to_owned()))
        }
    }

    pub fn leave_group_v2(
        &self,
        master: &MasterKey,
        group_id: &str,
        device_id: &str,
        pin: &str,
        now_ms: i64,
    ) -> LumoResult<()> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        if let Err(error) = verify_group_pin(&transaction, master, group_id, device_id, pin, now_ms)
        {
            transaction.commit().map_err(storage_error)?;
            return Err(error);
        }
        let role: String = transaction
            .query_row(
                "SELECT role FROM devices_v2
                 WHERE id = ?1 AND group_id = ?2 AND revoked_at_ms IS NULL",
                params![device_id, group_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?
            .ok_or(LumoError::Unauthorized)?;
        let changed = if DeviceRole::from_str(&role)? == DeviceRole::Controller {
            let active_controllers: u32 = transaction
                .query_row(
                    "SELECT COUNT(*) FROM devices_v2
                     WHERE group_id = ?1 AND role = 'controller' AND revoked_at_ms IS NULL",
                    params![group_id],
                    |row| row.get(0),
                )
                .map_err(storage_error)?;
            if active_controllers <= 1 {
                transaction
                    .execute("DELETE FROM groups_v2 WHERE id = ?1", params![group_id])
                    .map_err(storage_error)?
            } else {
                transaction
                    .execute(
                        "UPDATE devices_v2 SET revoked_at_ms = ?3
                         WHERE id = ?1 AND group_id = ?2 AND revoked_at_ms IS NULL",
                        params![device_id, group_id, now_ms],
                    )
                    .map_err(storage_error)?
            }
        } else {
            transaction
                .execute(
                    "UPDATE devices_v2 SET revoked_at_ms = ?3
                     WHERE id = ?1 AND group_id = ?2 AND revoked_at_ms IS NULL",
                    params![device_id, group_id, now_ms],
                )
                .map_err(storage_error)?
        };
        if changed != 1 {
            return Err(LumoError::Unauthorized);
        }
        transaction.commit().map_err(storage_error)
    }

    pub fn delete_group_v2(
        &self,
        master: &MasterKey,
        group_id: &str,
        controller_device_id: &str,
        pin: &str,
        now_ms: i64,
    ) -> LumoResult<()> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        if let Err(error) = verify_group_pin(
            &transaction,
            master,
            group_id,
            controller_device_id,
            pin,
            now_ms,
        ) {
            transaction.commit().map_err(storage_error)?;
            return Err(error);
        }
        let changed = transaction
            .execute("DELETE FROM groups_v2 WHERE id = ?1", params![group_id])
            .map_err(storage_error)?;
        if changed != 1 {
            return Err(LumoError::NotFound("group".to_owned()));
        }
        transaction.commit().map_err(storage_error)
    }

    fn lock(&self) -> LumoResult<std::sync::MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| LumoError::Storage("API database lock poisoned".to_owned()))
    }
}

fn insert_device(
    transaction: &Transaction<'_>,
    group_id: &str,
    device: &NewDevice,
    now_ms: i64,
) -> LumoResult<()> {
    let member_key_present =
        device.member_key_nonce.is_some() && device.member_key_ciphertext.is_some();
    if (device.role == DeviceRole::Controlled) != member_key_present {
        return Err(LumoError::InvalidInput(
            "controlled devices require a dedicated member key".to_owned(),
        ));
    }
    transaction
        .execute(
            "INSERT INTO devices_v2(
                id, group_id, name, role, token_hash, member_key_nonce,
                member_key_ciphertext, created_at_ms, last_seen_at_ms
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            params![
                device.id,
                group_id,
                device.name,
                device.role.as_str(),
                device.token_hash,
                device.member_key_nonce,
                device.member_key_ciphertext,
                now_ms
            ],
        )
        .map_err(storage_error)?;
    Ok(())
}

fn update_last_seen(connection: &Connection, device_id: &str, now_ms: i64) -> LumoResult<()> {
    connection
        .execute(
            "UPDATE devices_v2 SET last_seen_at_ms = ?2
             WHERE id = ?1 AND last_seen_at_ms <= ?3",
            params![
                device_id,
                now_ms,
                now_ms.saturating_sub(LAST_SEEN_WRITE_INTERVAL_MS)
            ],
        )
        .map_err(storage_error)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn authenticate_transaction(
    transaction: &Transaction<'_>,
    master: &MasterKey,
    group_id: &str,
    auth: &DeviceAuthAttempt,
    now_ms: i64,
    persist_nonce: bool,
    required_role: Option<DeviceRole>,
) -> LumoResult<AuthenticatedDevice> {
    let row: Option<(String, Vec<u8>, Option<i64>)> = transaction
        .query_row(
            "SELECT role, token_hash, revoked_at_ms
             FROM devices_v2 WHERE id = ?1 AND group_id = ?2",
            params![auth.device_id, group_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(storage_error)?;
    let (role, token_hash, revoked_at_ms) = row.ok_or(LumoError::AuthenticationFailed)?;
    let role = DeviceRole::from_str(&role)?;
    if revoked_at_ms.is_some() || !master.verify_token_hash(&auth.token, &token_hash) {
        return Err(LumoError::AuthenticationFailed);
    }
    if required_role.is_some_and(|required| required != role) {
        return Err(LumoError::Unauthorized);
    }
    if persist_nonce {
        transaction
            .execute(
                "DELETE FROM device_nonces_v2 WHERE accepted_at_ms < ?1",
                params![now_ms.saturating_sub(NONCE_TTL_MS)],
            )
            .map_err(storage_error)?;
        let nonce_count: i64 = transaction
            .query_row(
                "SELECT COUNT(*) FROM device_nonces_v2 WHERE device_id = ?1",
                params![auth.device_id],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if nonce_count >= MAX_NONCES_PER_DEVICE {
            return Err(LumoError::RateLimited);
        }
        let inserted = transaction
            .execute(
                "INSERT OR IGNORE INTO device_nonces_v2(device_id, nonce, accepted_at_ms)
                 VALUES(?1, ?2, ?3)",
                params![auth.device_id, auth.nonce, now_ms],
            )
            .map_err(storage_error)?;
        if inserted == 0 {
            return Err(LumoError::ReplayDetected);
        }
    }
    update_last_seen(transaction, &auth.device_id, now_ms)?;
    Ok(AuthenticatedDevice {
        group_id: group_id.to_owned(),
        device_id: auth.device_id.clone(),
        role,
    })
}

fn load_member_key(
    transaction: &Transaction<'_>,
    master: &MasterKey,
    group_id: &str,
    device_id: &str,
) -> LumoResult<[u8; 32]> {
    let wrapped: Option<WrappedMemberKeyRow> = transaction
        .query_row(
            "SELECT member_key_nonce, member_key_ciphertext
             FROM devices_v2 WHERE id = ?1 AND group_id = ?2 AND role = 'controlled'
               AND revoked_at_ms IS NULL",
            params![device_id, group_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(storage_error)?;
    let (Some(nonce), Some(ciphertext)) = wrapped.ok_or(LumoError::AuthenticationFailed)? else {
        return Err(LumoError::Storage(
            "controlled device member key is missing".to_owned(),
        ));
    };
    master.unwrap_member_key(group_id, device_id, &nonce, &ciphertext)
}

fn load_canonical_key(
    transaction: &Transaction<'_>,
    master: &MasterKey,
    group_id: &str,
) -> LumoResult<[u8; 32]> {
    let wrapped: (Vec<u8>, Vec<u8>) = transaction
        .query_row(
            "SELECT state_key_nonce, state_key_ciphertext FROM groups_v2 WHERE id = ?1",
            params![group_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(storage_error)?;
    master.unwrap_state_key(group_id, &wrapped.0, &wrapped.1)
}

fn load_runtime_state(
    transaction: &Transaction<'_>,
    master: &MasterKey,
    group_id: &str,
    now_ms: i64,
) -> LumoResult<Option<(RuntimeState, [u8; 32], RemoteStateRecord)>> {
    ensure_group(transaction, group_id)?;
    let row: Option<(Vec<u8>, Vec<u8>, Vec<u8>)> = transaction
        .query_row(
            "SELECT g.state_key_nonce, g.state_key_ciphertext, s.payload
             FROM groups_v2 g JOIN group_state_v2 s ON s.group_id = g.id
             WHERE g.id = ?1",
            params![group_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(storage_error)?;
    let Some((key_nonce, key_ciphertext, encoded)) = row else {
        return Ok(None);
    };
    let record = decode_record(Some(encoded))?.ok_or_else(|| {
        LumoError::Storage("persisted state record unexpectedly missing".to_owned())
    })?;
    let canonical_key = master.unwrap_state_key(group_id, &key_nonce, &key_ciphertext)?;
    let state: RuntimeState = SessionCipher::from_key(canonical_key).open(
        &record.envelope,
        now_ms,
        &mut ReplayGuard::default(),
    )?;
    if state.revision != record.revision {
        return Err(LumoError::Storage(
            "persisted state revision does not match its envelope".to_owned(),
        ));
    }
    Ok(Some((state, canonical_key, record)))
}

fn persist_state(
    transaction: &Transaction<'_>,
    group_id: &str,
    record: &RemoteStateRecord,
    now_ms: i64,
) -> LumoResult<()> {
    record.validate()?;
    let encoded =
        serde_json::to_vec(record).map_err(|error| LumoError::Serialization(error.to_string()))?;
    let changed = transaction
        .execute(
            "UPDATE group_state_v2 SET revision = ?2, payload = ?3
             WHERE group_id = ?1 AND revision < ?2",
            params![group_id, record.revision, encoded],
        )
        .map_err(storage_error)?;
    if changed != 1 {
        return Err(LumoError::RevisionConflict);
    }
    transaction
        .execute(
            "UPDATE groups_v2 SET initialized_at_ms = COALESCE(initialized_at_ms, ?2)
             WHERE id = ?1",
            params![group_id, now_ms],
        )
        .map_err(storage_error)?;
    Ok(())
}

fn validate_member_envelope(envelope: &SealedPayload, now_ms: i64) -> LumoResult<()> {
    let ttl_ms = envelope.expires_at_ms.saturating_sub(envelope.issued_at_ms);
    if ttl_ms <= 0
        || ttl_ms > MEMBER_ENVELOPE_TTL_MS
        || envelope.issued_at_ms > now_ms.saturating_add(NONCE_TTL_MS)
        || envelope.expires_at_ms < now_ms
    {
        return Err(LumoError::ExpiredMessage);
    }
    Ok(())
}

fn apply_controlled_operation(
    state: &mut RuntimeState,
    operation: ControlledOperation,
    now_ms: i64,
) -> LumoResult<Option<usize>> {
    let service = LumoService;
    match operation {
        ControlledOperation::SetTracking(input) => {
            service.set_tracking(state, input, now_ms)?;
            Ok(None)
        }
        ControlledOperation::ReportLocation(input) => {
            service.report_location(state, input, now_ms)?;
            Ok(None)
        }
        ControlledOperation::SetConnectivity { connectivity } => {
            service.set_connectivity(state, connectivity, now_ms)?;
            Ok(None)
        }
        ControlledOperation::SendHelp => {
            service.send_help(state, now_ms)?;
            Ok(None)
        }
        ControlledOperation::ProcessPending => service.process_pending(state, now_ms).map(Some),
    }
}

fn load_idempotent<T: DeserializeOwned>(
    connection: &Connection,
    master: &MasterKey,
    kind: &str,
    request_id: &str,
    request_digest: &[u8],
    not_before_ms: i64,
) -> LumoResult<Option<Idempotent<T>>> {
    let row: Option<StoredReplayRow> = connection
        .query_row(
            "SELECT request_digest, response_nonce, response_ciphertext, created_at_ms
             FROM idempotency_v2 WHERE kind = ?1 AND request_id = ?2",
            params![kind, request_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(storage_error)?;
    let Some((stored_digest, nonce, ciphertext, created_at_ms)) = row else {
        return Ok(None);
    };
    if created_at_ms < not_before_ms {
        return Ok(None);
    }
    if stored_digest != request_digest {
        return Ok(Some(Idempotent::Conflict));
    }
    let plaintext =
        master.open_replay_response(kind, request_id, request_digest, &nonce, &ciphertext)?;
    let value = serde_json::from_slice(&plaintext)
        .map_err(|error| LumoError::Storage(format!("invalid replay response: {error}")))?;
    Ok(Some(Idempotent::Replay(value)))
}

#[allow(clippy::too_many_arguments)]
fn store_idempotent<T: Serialize>(
    transaction: &Transaction<'_>,
    master: &MasterKey,
    kind: &str,
    request_id: &str,
    request_digest: &[u8],
    response: &T,
    now_ms: i64,
) -> LumoResult<()> {
    let plaintext = Zeroizing::new(
        serde_json::to_vec(response)
            .map_err(|error| LumoError::Serialization(error.to_string()))?,
    );
    let (nonce, ciphertext) =
        master.seal_replay_response(kind, request_id, request_digest, &plaintext)?;
    transaction
        .execute(
            "INSERT INTO idempotency_v2(
                kind, request_id, request_digest, response_nonce, response_ciphertext,
                created_at_ms
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![kind, request_id, request_digest, nonce, ciphertext, now_ms],
        )
        .map_err(storage_error)?;
    Ok(())
}

fn load_member_operation(
    transaction: &Transaction<'_>,
    master: &MasterKey,
    device_id: &str,
    operation_id: &str,
    request_digest: &[u8],
) -> LumoResult<Option<Idempotent<ControlledOperationResponse>>> {
    let row: Option<(Vec<u8>, Vec<u8>, Vec<u8>)> = transaction
        .query_row(
            "SELECT request_digest, response_nonce, response_ciphertext
             FROM member_operations_v2 WHERE device_id = ?1 AND operation_id = ?2",
            params![device_id, operation_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(storage_error)?;
    let Some((stored_digest, nonce, ciphertext)) = row else {
        return Ok(None);
    };
    if stored_digest != request_digest {
        return Ok(Some(Idempotent::Conflict));
    }
    let plaintext = master.open_replay_response(
        "member_operation",
        operation_id,
        request_digest,
        &nonce,
        &ciphertext,
    )?;
    let response = serde_json::from_slice(&plaintext)
        .map_err(|error| LumoError::Storage(format!("invalid operation replay: {error}")))?;
    Ok(Some(Idempotent::Replay(response)))
}

#[allow(clippy::too_many_arguments)]
fn store_member_operation(
    transaction: &Transaction<'_>,
    master: &MasterKey,
    device_id: &str,
    operation_id: &str,
    request_digest: &[u8],
    response: &ControlledOperationResponse,
    now_ms: i64,
) -> LumoResult<()> {
    let plaintext = Zeroizing::new(
        serde_json::to_vec(response)
            .map_err(|error| LumoError::Serialization(error.to_string()))?,
    );
    let (nonce, ciphertext) = master.seal_replay_response(
        "member_operation",
        operation_id,
        request_digest,
        &plaintext,
    )?;
    transaction
        .execute(
            "INSERT INTO member_operations_v2(
                device_id, operation_id, request_digest, response_nonce,
                response_ciphertext, created_at_ms
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                device_id,
                operation_id,
                request_digest,
                nonce,
                ciphertext,
                now_ms
            ],
        )
        .map_err(storage_error)?;
    Ok(())
}

fn add_column_if_missing(
    connection: &Connection,
    table: &str,
    column: &str,
    declaration: &str,
) -> LumoResult<()> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(storage_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    drop(statement);
    if !columns.iter().any(|existing| existing == column) {
        connection
            .execute(
                &format!("ALTER TABLE {table} ADD COLUMN {column} {declaration}"),
                [],
            )
            .map_err(storage_error)?;
    }
    Ok(())
}

fn ensure_group(transaction: &Transaction<'_>, group_id: &str) -> LumoResult<()> {
    let exists: bool = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM groups_v2 WHERE id = ?1)",
            params![group_id],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if exists {
        Ok(())
    } else {
        Err(LumoError::NotFound("group".to_owned()))
    }
}

fn decode_record(encoded: Option<Vec<u8>>) -> LumoResult<Option<RemoteStateRecord>> {
    let record = encoded
        .map(|bytes| {
            serde_json::from_slice::<RemoteStateRecord>(&bytes)
                .map_err(|error| LumoError::Storage(format!("invalid state record: {error}")))
        })
        .transpose()?;
    if let Some(record) = &record {
        record.validate().map_err(|_| {
            LumoError::Storage("persisted state record failed validation".to_owned())
        })?;
    }
    Ok(record)
}

fn verify_group_pin(
    transaction: &Transaction<'_>,
    master: &MasterKey,
    group_id: &str,
    device_id: &str,
    pin: &str,
    now_ms: i64,
) -> LumoResult<()> {
    let row: Option<(String, i64, Option<i64>)> = transaction
        .query_row(
            "SELECT g.pin_hash, COALESCE(p.failed_attempts, 0), p.locked_until_ms
             FROM groups_v2 g
             JOIN devices_v2 d ON d.group_id = g.id
             LEFT JOIN device_pin_guards_v2 p ON p.device_id = d.id
             WHERE g.id = ?1 AND d.id = ?2 AND d.revoked_at_ms IS NULL",
            params![group_id, device_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(storage_error)?;
    let (pin_hash, mut attempts, locked_until) = row.ok_or(LumoError::AuthenticationFailed)?;
    if locked_until.is_some_and(|until| until > now_ms) {
        return Err(LumoError::RateLimited);
    }
    if locked_until.is_some() {
        attempts = 0;
    }
    if master.verify_group_pin(group_id, pin, &pin_hash) {
        transaction
            .execute(
                "DELETE FROM device_pin_guards_v2 WHERE device_id = ?1",
                params![device_id],
            )
            .map_err(storage_error)?;
        return Ok(());
    }
    attempts = attempts.saturating_add(1);
    let next_lock = (attempts >= PIN_MAX_ATTEMPTS).then(|| now_ms.saturating_add(PIN_LOCK_MS));
    transaction
        .execute(
            "INSERT INTO device_pin_guards_v2(device_id, failed_attempts, locked_until_ms)
             VALUES(?1, ?2, ?3)
             ON CONFLICT(device_id) DO UPDATE SET
                failed_attempts = excluded.failed_attempts,
                locked_until_ms = excluded.locked_until_ms",
            params![device_id, attempts, next_lock],
        )
        .map_err(storage_error)?;
    Err(if next_lock.is_some() {
        LumoError::RateLimited
    } else {
        LumoError::Unauthorized
    })
}

fn consume_bootstrap_limit(
    transaction: &Transaction<'_>,
    key: &str,
    limit: u32,
    window_ms: i64,
    now_ms: i64,
) -> LumoResult<()> {
    let current: Option<(i64, u32)> = transaction
        .query_row(
            "SELECT window_started_ms, attempts FROM bootstrap_limits_v2 WHERE scope_key = ?1",
            params![key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(storage_error)?;
    let (window_started_ms, attempts) = match current {
        Some((started, attempts)) if now_ms.saturating_sub(started) < window_ms => {
            (started, attempts)
        }
        _ => (now_ms, 0),
    };
    if attempts >= limit {
        return Err(LumoError::RateLimited);
    }
    transaction
        .execute(
            "INSERT INTO bootstrap_limits_v2(scope_key, window_started_ms, attempts)
             VALUES(?1, ?2, ?3)
             ON CONFLICT(scope_key) DO UPDATE SET
                window_started_ms = excluded.window_started_ms,
                attempts = excluded.attempts",
            params![key, window_started_ms, attempts.saturating_add(1)],
        )
        .map_err(storage_error)?;
    Ok(())
}

fn cleanup(transaction: &Transaction<'_>, now_ms: i64, bootstrap_window_ms: i64) -> LumoResult<()> {
    cleanup_replays(transaction, now_ms)?;
    transaction
        .execute(
            "DELETE FROM invitations_v2 WHERE expires_at_ms < ?1 OR used_at_ms IS NOT NULL",
            params![now_ms],
        )
        .map_err(storage_error)?;
    transaction
        .execute(
            "DELETE FROM device_nonces_v2 WHERE accepted_at_ms < ?1",
            params![now_ms.saturating_sub(NONCE_TTL_MS)],
        )
        .map_err(storage_error)?;
    transaction
        .execute(
            "DELETE FROM groups_v2
             WHERE initialized_at_ms IS NULL AND created_at_ms < ?1",
            params![now_ms.saturating_sub(UNINITIALIZED_GROUP_TTL_MS)],
        )
        .map_err(storage_error)?;
    transaction
        .execute(
            "DELETE FROM devices_v2
             WHERE role = 'controlled' AND revoked_at_ms IS NOT NULL AND revoked_at_ms < ?1",
            params![now_ms.saturating_sub(REVOKED_DEVICE_TTL_MS)],
        )
        .map_err(storage_error)?;
    transaction
        .execute(
            "DELETE FROM bootstrap_limits_v2 WHERE window_started_ms < ?1",
            params![now_ms.saturating_sub(bootstrap_window_ms)],
        )
        .map_err(storage_error)?;
    Ok(())
}

fn cleanup_replays(transaction: &Transaction<'_>, now_ms: i64) -> LumoResult<()> {
    transaction
        .execute(
            "DELETE FROM bootstrap_requests_v2 WHERE created_at_ms < ?1",
            params![now_ms.saturating_sub(IDEMPOTENCY_TTL_MS)],
        )
        .map_err(storage_error)?;
    transaction
        .execute(
            "DELETE FROM idempotency_v2 WHERE created_at_ms < ?1",
            params![now_ms.saturating_sub(IDEMPOTENCY_TTL_MS)],
        )
        .map_err(storage_error)?;
    transaction
        .execute(
            "DELETE FROM member_operations_v2 WHERE created_at_ms < ?1",
            params![now_ms.saturating_sub(IDEMPOTENCY_TTL_MS)],
        )
        .map_err(storage_error)?;
    Ok(())
}
