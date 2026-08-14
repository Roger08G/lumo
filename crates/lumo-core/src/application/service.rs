use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    domain::{
        AppSnapshot, CommandKind, CommandStatus, Connectivity, EventKind, Group, Invitation,
        LocationSample, PendingCommand, PermissionState, Place, PlaceIcon, PlaceKind, PlaceTone,
        RuntimeProfile, RuntimeState, TimelineEvent, TripSummary, EVENT_TTL_MS,
    },
    geofence::containing_place,
    security::{hash_pin, validate_pin, verify_pin},
    LumoError, LumoResult,
};

const INVITE_TTL_MS: i64 = 10 * 60 * 1_000;
const MAX_PIN_ATTEMPTS: u8 = 5;
const PIN_LOCK_MS: i64 = 5 * 60 * 1_000;
const BATTERY_WARNING_COOLDOWN_MS: i64 = 60 * 60 * 1_000;
const BATTERY_WARNING_TITLE: &str = "Batería baja";
const MAX_COMMANDS: usize = 100;
const MAX_LOCATION_FUTURE_SKEW_MS: i64 = 5 * 60 * 1_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateGroupInput {
    pub name: String,
    pub supervisor_name: String,
    pub supervisor_phone: String,
    pub tracked_person_name: String,
    pub tracked_person_phone: String,
    pub pin: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePlaceInput {
    pub name: String,
    pub address: String,
    pub latitude: f64,
    pub longitude: f64,
    pub radius_m: u16,
    pub kind: PlaceKind,
    pub color: PlaceTone,
    pub icon: PlaceIcon,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportLocationInput {
    pub latitude: f64,
    pub longitude: f64,
    pub accuracy_m: f32,
    pub battery_percent: u8,
    pub captured_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetTrackingInput {
    pub precise_permission: PermissionState,
    pub background_permission: PermissionState,
    pub battery_optimization_disabled: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvitationView {
    pub invitation_id: String,
    pub token: String,
    pub group_name: String,
    pub group_code: String,
    pub expires_at_ms: i64,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct LumoService;

impl LumoService {
    pub fn snapshot(
        &self,
        state: &mut RuntimeState,
        profile: RuntimeProfile,
        now_ms: i64,
    ) -> AppSnapshot {
        purge_expired(state, now_ms);
        state.snapshot(profile)
    }

    pub fn create_group(
        &self,
        state: &mut RuntimeState,
        input: CreateGroupInput,
        now_ms: i64,
    ) -> LumoResult<()> {
        if state.group.is_some() {
            return Err(LumoError::InvalidInput(
                "a group already exists in this runtime".to_owned(),
            ));
        }
        let name = required_text("group name", &input.name)?;
        let supervisor_name = required_text("supervisor name", &input.supervisor_name)?;
        let supervisor_phone = validated_phone("supervisor phone", &input.supervisor_phone)?;
        let tracked_person_name = required_text("tracked person name", &input.tracked_person_name)?;
        let tracked_person_phone =
            validated_phone("tracked person phone", &input.tracked_person_phone)?;
        validate_pin(&input.pin)?;

        let current_revision = state.revision;
        *state = RuntimeState::default();
        state.revision = current_revision;
        state.group = Some(Group {
            id: Uuid::new_v4().to_string(),
            name,
            code: random_group_code(),
            supervisor_name,
            supervisor_phone,
            tracked_person_name,
            tracked_person_phone,
            pin_hash: hash_pin(&input.pin)?,
            created_at_ms: now_ms,
        });
        add_event(
            state,
            EventKind::System,
            "Grupo protegido creado",
            "La autoridad local de Rust está activa",
            None,
            now_ms,
        );
        bump_revision(state);
        Ok(())
    }

    pub fn verify_protected_action(
        &self,
        state: &mut RuntimeState,
        pin: &str,
        now_ms: i64,
    ) -> LumoResult<()> {
        validate_pin(pin)?;
        if let Some(locked_until) = state.pin_guard.locked_until_ms {
            if now_ms < locked_until {
                return Err(LumoError::RateLimited);
            }
            state.pin_guard.failed_attempts = 0;
            state.pin_guard.locked_until_ms = None;
        }

        let group = state.group.as_ref().ok_or(LumoError::GroupNotInitialized)?;
        if verify_pin(pin, &group.pin_hash) {
            state.pin_guard.failed_attempts = 0;
            state.pin_guard.locked_until_ms = None;
            return Ok(());
        }

        state.pin_guard.failed_attempts = state.pin_guard.failed_attempts.saturating_add(1);
        if state.pin_guard.failed_attempts >= MAX_PIN_ATTEMPTS {
            state.pin_guard.locked_until_ms = Some(now_ms.saturating_add(PIN_LOCK_MS));
        }
        bump_revision(state);
        Err(LumoError::Unauthorized)
    }

    pub fn create_invitation(
        &self,
        state: &mut RuntimeState,
        pin: &str,
        now_ms: i64,
    ) -> LumoResult<InvitationView> {
        self.verify_protected_action(state, pin, now_ms)?;
        let group = state.group.as_ref().ok_or(LumoError::GroupNotInitialized)?;
        let mut token_bytes = [0_u8; 32];
        OsRng.fill_bytes(&mut token_bytes);
        let token = URL_SAFE_NO_PAD.encode(token_bytes);
        let invitation_id = Uuid::new_v4().to_string();
        let expires_at_ms = now_ms.saturating_add(INVITE_TTL_MS);
        let view = InvitationView {
            invitation_id: invitation_id.clone(),
            token: token.clone(),
            group_name: group.name.clone(),
            group_code: group.code.clone(),
            expires_at_ms,
        };
        state.invitations.push(Invitation {
            id: invitation_id,
            token_hash: token_digest(&token),
            expires_at_ms,
            used_at_ms: None,
        });
        bump_revision(state);
        Ok(view)
    }

    pub fn leave_group(&self, state: &mut RuntimeState, pin: &str, now_ms: i64) -> LumoResult<()> {
        self.verify_protected_action(state, pin, now_ms)?;
        let current_revision = state.revision;
        *state = RuntimeState::default();
        state.revision = current_revision;
        bump_revision(state);
        Ok(())
    }

    pub fn consume_invitation(
        &self,
        state: &mut RuntimeState,
        token: &str,
        pin: &str,
        now_ms: i64,
    ) -> LumoResult<()> {
        self.verify_protected_action(state, pin, now_ms)?;
        let digest = token_digest(token);
        let invitation = state
            .invitations
            .iter_mut()
            .find(|invite| invite.token_hash == digest)
            .ok_or(LumoError::InvalidInvitation)?;
        if invitation.used_at_ms.is_some() || invitation.expires_at_ms < now_ms {
            return Err(LumoError::InvalidInvitation);
        }
        invitation.used_at_ms = Some(now_ms);
        bump_revision(state);
        Ok(())
    }

    pub fn create_place(
        &self,
        state: &mut RuntimeState,
        input: CreatePlaceInput,
        now_ms: i64,
    ) -> LumoResult<Place> {
        ensure_group(state)?;
        validate_place(&input)?;
        let place = Place {
            id: Uuid::new_v4().to_string(),
            name: input.name.trim().to_owned(),
            address: input.address.trim().to_owned(),
            latitude: input.latitude,
            longitude: input.longitude,
            radius_m: input.radius_m,
            kind: input.kind,
            color: input.color,
            icon: input.icon,
        };
        state.places.push(place.clone());
        add_event(
            state,
            EventKind::System,
            "Lugar añadido",
            &place.name,
            Some(place.id.clone()),
            now_ms,
        );
        bump_revision(state);
        Ok(place)
    }

    pub fn update_place(
        &self,
        state: &mut RuntimeState,
        id: &str,
        input: CreatePlaceInput,
        now_ms: i64,
    ) -> LumoResult<Place> {
        ensure_group(state)?;
        validate_place(&input)?;
        let place = state
            .places
            .iter_mut()
            .find(|place| place.id == id)
            .ok_or_else(|| LumoError::NotFound("place".to_owned()))?;
        place.name = input.name.trim().to_owned();
        place.address = input.address.trim().to_owned();
        place.latitude = input.latitude;
        place.longitude = input.longitude;
        place.radius_m = input.radius_m;
        place.kind = input.kind;
        place.color = input.color;
        place.icon = input.icon;
        let updated = place.clone();
        add_event(
            state,
            EventKind::System,
            "Lugar actualizado",
            &updated.name,
            Some(updated.id.clone()),
            now_ms,
        );
        bump_revision(state);
        Ok(updated)
    }

    pub fn delete_place(
        &self,
        state: &mut RuntimeState,
        id: &str,
        pin: &str,
        now_ms: i64,
    ) -> LumoResult<()> {
        self.verify_protected_action(state, pin, now_ms)?;
        let position = state
            .places
            .iter()
            .position(|place| place.id == id)
            .ok_or_else(|| LumoError::NotFound("place".to_owned()))?;
        let place = state.places.remove(position);
        if state.controlled.current_place_id.as_deref() == Some(id) {
            state.controlled.current_place_id = None;
        }
        add_event(
            state,
            EventKind::System,
            "Lugar eliminado",
            &place.name,
            Some(place.id),
            now_ms,
        );
        bump_revision(state);
        Ok(())
    }

    pub fn set_tracking(
        &self,
        state: &mut RuntimeState,
        input: SetTrackingInput,
        now_ms: i64,
    ) -> LumoResult<()> {
        ensure_group(state)?;
        if input.enabled
            && (input.precise_permission != PermissionState::Granted
                || input.background_permission != PermissionState::Granted)
        {
            return Err(LumoError::InvalidInput(
                "tracking requires precise and background location permissions".to_owned(),
            ));
        }
        state.controlled.precise_permission = input.precise_permission;
        state.controlled.background_permission = input.background_permission;
        state.controlled.battery_optimization_disabled = input.battery_optimization_disabled;
        state.controlled.tracking_enabled = input.enabled;
        state.controlled.last_seen_at_ms = Some(now_ms);
        bump_revision(state);
        Ok(())
    }

    pub fn set_connectivity(
        &self,
        state: &mut RuntimeState,
        connectivity: Connectivity,
        now_ms: i64,
    ) -> LumoResult<()> {
        ensure_group(state)?;
        if state.controlled.connectivity == connectivity {
            return Ok(());
        }
        state.controlled.connectivity = connectivity;
        match connectivity {
            Connectivity::Online => state.controlled.last_seen_at_ms = Some(now_ms),
            Connectivity::Offline => {
                add_event(
                    state,
                    EventKind::Warning,
                    "Conexión interrumpida",
                    "El teléfono controlado está sin conexión",
                    None,
                    now_ms,
                );
            }
        }
        bump_revision(state);
        Ok(())
    }

    pub fn report_location(
        &self,
        state: &mut RuntimeState,
        input: ReportLocationInput,
        now_ms: i64,
    ) -> LumoResult<()> {
        ensure_group(state)?;
        validate_location(&input)?;
        if !state.controlled.tracking_enabled
            || state.controlled.precise_permission != PermissionState::Granted
            || state.controlled.background_permission != PermissionState::Granted
        {
            return Err(LumoError::TrackingDisabled);
        }

        let captured_at_ms = input.captured_at_ms.unwrap_or(now_ms);
        // An offline queue can cross the 24-hour retention boundary between
        // reading and uploading a sample. Treat that sample as acknowledged
        // and obsolete so it cannot block every newer queued location.
        if captured_at_ms < now_ms.saturating_sub(EVENT_TTL_MS) {
            return Ok(());
        }
        if captured_at_ms > now_ms.saturating_add(MAX_LOCATION_FUTURE_SKEW_MS) {
            return Err(LumoError::InvalidInput(
                "location timestamp is outside the accepted window".to_owned(),
            ));
        }
        if state
            .controlled
            .last_location
            .as_ref()
            .is_some_and(|location| location.captured_at_ms >= captured_at_ms)
        {
            return Ok(());
        }
        let next_place = containing_place(&state.places, input.latitude, input.longitude)
            .map(|place| (place.id.clone(), place.name.clone()));
        let previous_place_id = state.controlled.current_place_id.clone();

        if previous_place_id != next_place.as_ref().map(|(id, _)| id.clone()) {
            if let Some(previous_id) = previous_place_id.as_deref() {
                let previous_name = place_name(state, previous_id);
                add_event(
                    state,
                    EventKind::Departure,
                    "Ha salido de un lugar habitual",
                    &previous_name,
                    Some(previous_id.to_owned()),
                    captured_at_ms,
                );
                state.controlled.departed_place_id = Some(previous_id.to_owned());
                state.controlled.departed_at_ms = Some(captured_at_ms);
            }
            if let Some((next_id, next_name)) = next_place.as_ref() {
                add_event(
                    state,
                    EventKind::Arrival,
                    "Ha llegado a un lugar habitual",
                    next_name,
                    Some(next_id.clone()),
                    captured_at_ms,
                );
                if let (Some(origin_id), Some(started_at_ms)) = (
                    state.controlled.departed_place_id.take(),
                    state.controlled.departed_at_ms.take(),
                ) {
                    state.controlled.last_trip = Some(TripSummary {
                        from: place_name(state, &origin_id),
                        to: next_name.clone(),
                        started_at_ms,
                        ended_at_ms: captured_at_ms,
                        duration_seconds: u64::try_from(
                            captured_at_ms.saturating_sub(started_at_ms).max(0),
                        )
                        .unwrap_or_default()
                            / 1_000,
                    });
                }
            }
        }

        state.controlled.current_place_id = next_place.map(|(id, _)| id);
        state.controlled.last_location = Some(LocationSample {
            latitude: input.latitude,
            longitude: input.longitude,
            accuracy_m: input.accuracy_m,
            captured_at_ms,
            battery_percent: input.battery_percent,
        });
        state.controlled.battery_percent = input.battery_percent;
        state.controlled.connectivity = Connectivity::Online;
        state.controlled.last_seen_at_ms = Some(now_ms);
        if input.battery_percent <= 15
            && !has_recent_event(
                state,
                EventKind::Warning,
                BATTERY_WARNING_TITLE,
                now_ms,
                BATTERY_WARNING_COOLDOWN_MS,
            )
        {
            add_event(
                state,
                EventKind::Warning,
                BATTERY_WARNING_TITLE,
                &format!("Queda un {} %", input.battery_percent),
                None,
                now_ms,
            );
        }
        complete_locate_commands(state, now_ms);
        purge_expired(state, now_ms);
        bump_revision(state);
        Ok(())
    }

    pub fn request_location(&self, state: &mut RuntimeState, now_ms: i64) -> LumoResult<String> {
        ensure_group(state)?;
        purge_expired(state, now_ms);
        if let Some(command) = state.commands.iter().find(|command| {
            command.kind == CommandKind::Locate && command.status == CommandStatus::Queued
        }) {
            return Ok(command.id.clone());
        }
        let id = Uuid::new_v4().to_string();
        state.commands.push(PendingCommand {
            id: id.clone(),
            kind: CommandKind::Locate,
            status: CommandStatus::Queued,
            created_at_ms: now_ms,
            completed_at_ms: None,
            error_code: None,
        });
        trim_commands(state);
        bump_revision(state);
        Ok(id)
    }

    pub fn process_pending(&self, state: &mut RuntimeState, now_ms: i64) -> LumoResult<usize> {
        ensure_group(state)?;
        let queued = state
            .commands
            .iter()
            .filter(|command| command.status == CommandStatus::Queued)
            .count();
        if state.controlled.last_location.is_some() {
            complete_locate_commands(state, now_ms);
        } else {
            for command in state
                .commands
                .iter_mut()
                .filter(|command| command.status == CommandStatus::Queued)
            {
                command.status = CommandStatus::Failed;
                command.completed_at_ms = Some(now_ms);
                command.error_code = Some("location_unavailable".to_owned());
            }
        }
        bump_revision(state);
        Ok(queued)
    }

    pub fn send_help(&self, state: &mut RuntimeState, now_ms: i64) -> LumoResult<()> {
        ensure_group(state)?;
        add_event(
            state,
            EventKind::Help,
            "Necesita ayuda",
            "La persona controlada ha solicitado contacto",
            state.controlled.current_place_id.clone(),
            now_ms,
        );
        bump_revision(state);
        Ok(())
    }

    pub fn mark_events_read(&self, state: &mut RuntimeState, now_ms: i64) -> LumoResult<()> {
        ensure_group(state)?;
        for event in &mut state.events {
            event.read_at_ms.get_or_insert(now_ms);
        }
        bump_revision(state);
        Ok(())
    }

    pub fn reset(&self, state: &mut RuntimeState) {
        let current_revision = state.revision;
        *state = RuntimeState::default();
        state.revision = current_revision;
        bump_revision(state);
    }
}

fn ensure_group(state: &RuntimeState) -> LumoResult<&Group> {
    state.group.as_ref().ok_or(LumoError::GroupNotInitialized)
}

fn required_text(label: &str, value: &str) -> LumoResult<String> {
    let value = value.trim();
    if (2..=80).contains(&value.chars().count()) {
        Ok(value.to_owned())
    } else {
        Err(LumoError::InvalidInput(format!(
            "{label} must contain between 2 and 80 characters"
        )))
    }
}

fn validated_phone(label: &str, value: &str) -> LumoResult<String> {
    let value = value.trim();
    let digits = value.chars().filter(char::is_ascii_digit).count();
    let valid_characters = value.chars().all(|character| {
        character.is_ascii_digit() || matches!(character, '+' | ' ' | '-' | '(' | ')')
    });
    let plus_is_valid = value.chars().filter(|character| *character == '+').count()
        == usize::from(value.starts_with('+'));
    if valid_characters && plus_is_valid && (7..=15).contains(&digits) {
        Ok(value.to_owned())
    } else {
        Err(LumoError::InvalidInput(format!(
            "{label} must contain a valid phone number"
        )))
    }
}

fn validate_place(input: &CreatePlaceInput) -> LumoResult<()> {
    required_text("place name", &input.name)?;
    required_text("address", &input.address)?;
    if !input.latitude.is_finite() || !(-90.0..=90.0).contains(&input.latitude) {
        return Err(LumoError::InvalidInput("invalid latitude".to_owned()));
    }
    if !input.longitude.is_finite() || !(-180.0..=180.0).contains(&input.longitude) {
        return Err(LumoError::InvalidInput("invalid longitude".to_owned()));
    }
    if !(25..=2_000).contains(&input.radius_m) {
        return Err(LumoError::InvalidInput(
            "radius must be between 25 and 2000 metres".to_owned(),
        ));
    }
    Ok(())
}

fn validate_location(input: &ReportLocationInput) -> LumoResult<()> {
    if !input.latitude.is_finite() || !(-90.0..=90.0).contains(&input.latitude) {
        return Err(LumoError::InvalidInput("invalid latitude".to_owned()));
    }
    if !input.longitude.is_finite() || !(-180.0..=180.0).contains(&input.longitude) {
        return Err(LumoError::InvalidInput("invalid longitude".to_owned()));
    }
    if !input.accuracy_m.is_finite() || input.accuracy_m < 0.0 || input.accuracy_m > 10_000.0 {
        return Err(LumoError::InvalidInput("invalid accuracy".to_owned()));
    }
    if input.battery_percent > 100 {
        return Err(LumoError::InvalidInput(
            "battery percent must be between 0 and 100".to_owned(),
        ));
    }
    Ok(())
}

fn token_digest(token: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(token.as_bytes()))
}

fn random_group_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut random = [0_u8; 8];
    OsRng.fill_bytes(&mut random);
    let suffix = random
        .iter()
        .map(|byte| ALPHABET[usize::from(*byte) % ALPHABET.len()] as char)
        .collect::<String>();
    format!("LUMO-{suffix}")
}

fn add_event(
    state: &mut RuntimeState,
    kind: EventKind,
    title: &str,
    detail: &str,
    place_id: Option<String>,
    occurred_at_ms: i64,
) {
    let event = TimelineEvent {
        id: Uuid::new_v4().to_string(),
        sequence: state.next_sequence,
        kind,
        occurred_at_ms,
        title: title.to_owned(),
        detail: detail.to_owned(),
        place_id,
        read_at_ms: None,
    };
    state.next_sequence = state.next_sequence.saturating_add(1);
    state.events.insert(0, event);
    state.events.truncate(200);
}

fn has_recent_event(
    state: &RuntimeState,
    kind: EventKind,
    title: &str,
    now_ms: i64,
    cooldown_ms: i64,
) -> bool {
    state.events.iter().any(|event| {
        event.kind == kind
            && event.title == title
            && now_ms.saturating_sub(event.occurred_at_ms) < cooldown_ms
    })
}

fn purge_expired(state: &mut RuntimeState, now_ms: i64) {
    state
        .events
        .retain(|event| now_ms.saturating_sub(event.occurred_at_ms) < EVENT_TTL_MS);
    state
        .invitations
        .retain(|invite| invite.used_at_ms.is_none() && invite.expires_at_ms >= now_ms);
    state.commands.retain(|command| {
        command.status == CommandStatus::Queued
            || now_ms.saturating_sub(command.completed_at_ms.unwrap_or(command.created_at_ms))
                < EVENT_TTL_MS
    });
    trim_commands(state);
}

fn trim_commands(state: &mut RuntimeState) {
    let completed_to_remove = state.commands.len().saturating_sub(MAX_COMMANDS);
    if completed_to_remove == 0 {
        return;
    }

    let mut remaining = completed_to_remove;
    state.commands.retain(|command| {
        if remaining > 0 && command.status != CommandStatus::Queued {
            remaining -= 1;
            false
        } else {
            true
        }
    });
}

fn place_name(state: &RuntimeState, id: &str) -> String {
    state
        .places
        .iter()
        .find(|place| place.id == id)
        .map(|place| place.name.clone())
        .unwrap_or_else(|| "Ubicación desconocida".to_owned())
}

fn complete_locate_commands(state: &mut RuntimeState, now_ms: i64) {
    for command in state
        .commands
        .iter_mut()
        .filter(|command| command.status == CommandStatus::Queued)
    {
        command.status = CommandStatus::Completed;
        command.completed_at_ms = Some(now_ms);
        command.error_code = None;
    }
}

fn bump_revision(state: &mut RuntimeState) {
    state.revision = state.revision.saturating_add(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group_input() -> CreateGroupInput {
        CreateGroupInput {
            name: "Grupo familiar".into(),
            supervisor_name: "Supervisor".into(),
            supervisor_phone: "+34600000001".into(),
            tracked_person_name: "Persona acompañada".into(),
            tracked_person_phone: "+34600000002".into(),
            pin: "123456".into(),
        }
    }

    fn place_input(name: &str, latitude: f64, longitude: f64) -> CreatePlaceInput {
        CreatePlaceInput {
            name: name.into(),
            address: "Dirección de prueba".into(),
            latitude,
            longitude,
            radius_m: 50,
            kind: PlaceKind::Place,
            color: PlaceTone::Purple,
            icon: PlaceIcon::Pin,
        }
    }

    #[test]
    fn snapshot_never_serializes_the_pin_hash() {
        let service = LumoService;
        let mut state = RuntimeState::default();
        service
            .create_group(&mut state, group_input(), 1)
            .expect("group");
        let json =
            serde_json::to_string(&state.snapshot(RuntimeProfile::Controller)).expect("json");
        assert!(!json.contains("123456"));
        assert!(!json.contains("argon2"));
        assert!(!json.contains("pinHash"));
    }

    #[test]
    fn controlled_member_snapshot_is_least_privilege() {
        let service = LumoService;
        let mut state = RuntimeState::default();
        service
            .create_group(&mut state, group_input(), 1)
            .expect("group");
        service
            .create_place(&mut state, place_input("Casa", 40.0, -3.0), 2)
            .expect("place");
        service
            .request_location(&mut state, 3)
            .expect("pending command");
        service
            .create_invitation(&mut state, "123456", 4)
            .expect("invitation");

        let snapshot = state.member_snapshot();
        assert_eq!(snapshot.profile, RuntimeProfile::Controlled);
        assert!(snapshot.session.is_some());
        assert!(snapshot.places.is_empty());
        assert!(snapshot.events.is_empty());
        assert!(snapshot.commands.is_empty());

        let json = serde_json::to_string(&snapshot).expect("json");
        for private_field in [
            "pinHash",
            "pinGuard",
            "invitations",
            "nextSequence",
            "123456",
            "argon2",
        ] {
            assert!(!json.contains(private_field), "leaked {private_field}");
        }
    }

    #[test]
    fn protected_actions_lock_after_repeated_failures() {
        let service = LumoService;
        let mut state = RuntimeState::default();
        service
            .create_group(&mut state, group_input(), 1)
            .expect("group");
        for _ in 0..MAX_PIN_ATTEMPTS {
            assert_eq!(
                service.verify_protected_action(&mut state, "000000", 10),
                Err(LumoError::Unauthorized)
            );
        }
        assert_eq!(
            service.verify_protected_action(&mut state, "123456", 11),
            Err(LumoError::RateLimited)
        );
        assert!(service
            .verify_protected_action(&mut state, "123456", 10 + PIN_LOCK_MS)
            .is_ok());
    }

    #[test]
    fn invitation_is_single_use() {
        let service = LumoService;
        let mut state = RuntimeState::default();
        service
            .create_group(&mut state, group_input(), 1)
            .expect("group");
        let invite = service
            .create_invitation(&mut state, "123456", 10)
            .expect("invite");
        service
            .consume_invitation(&mut state, &invite.token, "123456", 20)
            .expect("first use");
        assert_eq!(
            service.consume_invitation(&mut state, &invite.token, "123456", 21),
            Err(LumoError::InvalidInvitation)
        );
    }

    #[test]
    fn location_transitions_create_one_arrival_per_entry() {
        let service = LumoService;
        let mut state = RuntimeState::default();
        service
            .create_group(&mut state, group_input(), 1)
            .expect("group");
        service
            .create_place(&mut state, place_input("Casa", 40.4168, -3.7038), 2)
            .expect("place");
        service
            .set_tracking(
                &mut state,
                SetTrackingInput {
                    precise_permission: PermissionState::Granted,
                    background_permission: PermissionState::Granted,
                    battery_optimization_disabled: true,
                    enabled: true,
                },
                3,
            )
            .expect("tracking");
        let location = ReportLocationInput {
            latitude: 40.4168,
            longitude: -3.7038,
            accuracy_m: 8.0,
            battery_percent: 80,
            captured_at_ms: None,
        };
        service
            .report_location(&mut state, location.clone(), 4)
            .expect("first");
        service
            .report_location(&mut state, location, 5)
            .expect("second");
        assert_eq!(
            state
                .events
                .iter()
                .filter(|event| event.kind == EventKind::Arrival)
                .count(),
            1
        );
    }

    #[test]
    fn events_expire_after_twenty_four_hours() {
        let service = LumoService;
        let mut state = RuntimeState::default();
        service
            .create_group(&mut state, group_input(), 0)
            .expect("group");
        let snapshot = service.snapshot(&mut state, RuntimeProfile::Controller, EVENT_TTL_MS + 1);
        assert!(snapshot.events.is_empty());
    }

    #[test]
    fn battery_warnings_are_throttled_but_repeat_after_the_cooldown() {
        let service = LumoService;
        let mut state = RuntimeState::default();
        service
            .create_group(&mut state, group_input(), 1)
            .expect("group");
        service
            .set_tracking(
                &mut state,
                SetTrackingInput {
                    precise_permission: PermissionState::Granted,
                    background_permission: PermissionState::Granted,
                    battery_optimization_disabled: true,
                    enabled: true,
                },
                2,
            )
            .expect("tracking");
        let location = ReportLocationInput {
            latitude: 40.4168,
            longitude: -3.7038,
            accuracy_m: 8.0,
            battery_percent: 12,
            captured_at_ms: None,
        };

        service
            .report_location(&mut state, location.clone(), 3)
            .expect("first warning");
        service
            .report_location(&mut state, location.clone(), 4)
            .expect("throttled warning");
        service
            .report_location(&mut state, location, 3 + BATTERY_WARNING_COOLDOWN_MS)
            .expect("warning after cooldown");

        assert_eq!(
            state
                .events
                .iter()
                .filter(|event| event.title == BATTERY_WARNING_TITLE)
                .count(),
            2
        );
    }

    #[test]
    fn stale_location_samples_never_replace_a_newer_position() {
        let service = LumoService;
        let mut state = RuntimeState::default();
        service
            .create_group(&mut state, group_input(), 1)
            .expect("group");
        service
            .set_tracking(
                &mut state,
                SetTrackingInput {
                    precise_permission: PermissionState::Granted,
                    background_permission: PermissionState::Granted,
                    battery_optimization_disabled: true,
                    enabled: true,
                },
                2,
            )
            .expect("tracking");
        service
            .report_location(
                &mut state,
                ReportLocationInput {
                    latitude: 40.4168,
                    longitude: -3.7038,
                    accuracy_m: 8.0,
                    battery_percent: 80,
                    captured_at_ms: Some(10_000),
                },
                10_000,
            )
            .expect("current location");
        let revision = state.revision;

        service
            .report_location(
                &mut state,
                ReportLocationInput {
                    latitude: 41.0,
                    longitude: -4.0,
                    accuracy_m: 8.0,
                    battery_percent: 79,
                    captured_at_ms: Some(9_000),
                },
                11_000,
            )
            .expect("stale sample ignored");

        let location = state.controlled.last_location.expect("last location");
        assert_eq!(location.latitude, 40.4168);
        assert_eq!(location.captured_at_ms, 10_000);
        assert_eq!(state.revision, revision);
    }

    #[test]
    fn location_timestamp_must_be_recent_and_not_far_in_the_future() {
        let service = LumoService;
        let mut state = RuntimeState::default();
        service
            .create_group(&mut state, group_input(), 1)
            .expect("group");
        service
            .set_tracking(
                &mut state,
                SetTrackingInput {
                    precise_permission: PermissionState::Granted,
                    background_permission: PermissionState::Granted,
                    battery_optimization_disabled: true,
                    enabled: true,
                },
                2,
            )
            .expect("tracking");
        let input = |captured_at_ms| ReportLocationInput {
            latitude: 40.4168,
            longitude: -3.7038,
            accuracy_m: 8.0,
            battery_percent: 80,
            captured_at_ms: Some(captured_at_ms),
        };

        let revision = state.revision;
        service
            .report_location(&mut state, input(0), EVENT_TTL_MS + 1)
            .expect("expired offline sample is ignored");
        assert_eq!(state.revision, revision);
        assert!(matches!(
            service.report_location(
                &mut state,
                input(1_000 + MAX_LOCATION_FUTURE_SKEW_MS + 1),
                1_000,
            ),
            Err(LumoError::InvalidInput(_))
        ));
    }

    #[test]
    fn repeated_location_requests_reuse_the_queued_command() {
        let service = LumoService;
        let mut state = RuntimeState::default();
        service
            .create_group(&mut state, group_input(), 1)
            .expect("group");

        let first = service
            .request_location(&mut state, 2)
            .expect("first command");
        let revision = state.revision;
        let second = service
            .request_location(&mut state, 3)
            .expect("same queued command");

        assert_eq!(first, second);
        assert_eq!(state.commands.len(), 1);
        assert_eq!(state.revision, revision);
    }

    #[test]
    fn group_rejects_invalid_contact_numbers() {
        let service = LumoService;
        let mut state = RuntimeState::default();
        let mut input = group_input();
        input.supervisor_phone = "123".into();
        assert!(matches!(
            service.create_group(&mut state, input, 1),
            Err(LumoError::InvalidInput(_))
        ));

        let mut input = group_input();
        input.tracked_person_phone = "+34+600000002".into();
        assert!(matches!(
            service.create_group(&mut state, input, 1),
            Err(LumoError::InvalidInput(_))
        ));
    }

    #[test]
    fn repeated_offline_detection_creates_only_one_warning() {
        let service = LumoService;
        let mut state = RuntimeState::default();
        service
            .create_group(&mut state, group_input(), 1)
            .expect("group");
        service
            .set_connectivity(&mut state, Connectivity::Online, 2)
            .expect("online");
        service
            .set_connectivity(&mut state, Connectivity::Offline, 3)
            .expect("offline");
        service
            .set_connectivity(&mut state, Connectivity::Offline, 4)
            .expect("same offline state");
        assert_eq!(
            state
                .events
                .iter()
                .filter(|event| event.title == "Conexión interrumpida")
                .count(),
            1
        );
        assert_eq!(state.controlled.last_seen_at_ms, Some(2));
    }

    #[test]
    fn recreate_reset_and_leave_keep_revisions_monotonic() {
        let service = LumoService;
        let mut state = RuntimeState {
            revision: 41,
            ..RuntimeState::default()
        };

        service
            .create_group(&mut state, group_input(), 1)
            .expect("group");
        assert_eq!(state.revision, 42);

        service.reset(&mut state);
        assert_eq!(state.revision, 43);
        assert!(state.group.is_none());

        service
            .create_group(&mut state, group_input(), 2)
            .expect("group after reset");
        assert_eq!(state.revision, 44);

        service
            .leave_group(&mut state, "123456", 3)
            .expect("leave group");
        assert_eq!(state.revision, 45);
        assert!(state.group.is_none());
    }
}
