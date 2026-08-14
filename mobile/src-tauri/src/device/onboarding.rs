use std::{
    fmt, fs,
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

use lumo_core::{LumoError, LumoResult};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PendingKind {
    Create,
    Join,
    Leave,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingOnboarding {
    kind: PendingKind,
    request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    invitation_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PendingOnboardingStore {
    path: Arc<PathBuf>,
    lock: Arc<Mutex<()>>,
}

impl PendingOnboardingStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: Arc::new(path.into()),
            lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn begin_create(&self) -> LumoResult<String> {
        let _guard = self.guard()?;
        match self.load_unlocked()? {
            Some(pending) if pending.kind == PendingKind::Create => Ok(pending.request_id),
            Some(_) => Err(pending_conflict()),
            None => self.create_unlocked(PendingKind::Create, None),
        }
    }

    pub fn begin_join(&self, invitation_id: &str) -> LumoResult<String> {
        Uuid::parse_str(invitation_id)
            .map_err(|_| LumoError::InvalidInput("invalid invitation identifier".to_owned()))?;
        let _guard = self.guard()?;
        match self.load_unlocked()? {
            Some(pending)
                if pending.kind == PendingKind::Join
                    && pending.invitation_id.as_deref() == Some(invitation_id) =>
            {
                Ok(pending.request_id)
            }
            Some(_) => Err(pending_conflict()),
            None => self.create_unlocked(PendingKind::Join, Some(invitation_id.to_owned())),
        }
    }

    pub fn begin_leave(&self) -> LumoResult<String> {
        let _guard = self.guard()?;
        match self.load_unlocked()? {
            Some(pending) if pending.kind == PendingKind::Leave => Ok(pending.request_id),
            Some(_) => Err(pending_conflict()),
            None => self.create_unlocked(PendingKind::Leave, None),
        }
    }

    pub fn is_leave_pending(&self) -> LumoResult<bool> {
        let _guard = self.guard()?;
        Ok(self
            .load_unlocked()?
            .is_some_and(|pending| pending.kind == PendingKind::Leave))
    }

    pub fn confirm_onboarding(&self) -> LumoResult<()> {
        let _guard = self.guard()?;
        match fs::remove_file(self.path.as_ref()) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(storage_error(error)),
        }
    }

    fn create_unlocked(
        &self,
        kind: PendingKind,
        invitation_id: Option<String>,
    ) -> LumoResult<String> {
        let pending = PendingOnboarding {
            kind,
            request_id: Uuid::new_v4().to_string(),
            invitation_id,
        };
        self.store_unlocked(&pending)?;
        Ok(pending.request_id)
    }

    fn load_unlocked(&self) -> LumoResult<Option<PendingOnboarding>> {
        let bytes = match fs::read(self.path.as_ref()) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(storage_error(error)),
        };
        let pending: PendingOnboarding = serde_json::from_slice(&bytes)
            .map_err(|_| LumoError::Storage("invalid pending onboarding state".to_owned()))?;
        if Uuid::parse_str(&pending.request_id).is_err()
            || pending
                .invitation_id
                .as_deref()
                .is_some_and(|value| Uuid::parse_str(value).is_err())
            || (matches!(pending.kind, PendingKind::Create | PendingKind::Leave)
                && pending.invitation_id.is_some())
            || (pending.kind == PendingKind::Join && pending.invitation_id.is_none())
        {
            return Err(LumoError::Storage(
                "invalid pending onboarding state".to_owned(),
            ));
        }
        Ok(Some(pending))
    }

    fn store_unlocked(&self, pending: &PendingOnboarding) -> LumoResult<()> {
        let parent = self.path.parent().ok_or_else(|| {
            LumoError::Storage("pending onboarding path has no parent".to_owned())
        })?;
        fs::create_dir_all(parent).map_err(storage_error)?;
        let bytes = serde_json::to_vec(pending)
            .map_err(|error| LumoError::Serialization(error.to_string()))?;
        let temporary = parent.join(format!(".pending-onboarding-{}.tmp", Uuid::new_v4()));
        write_private(&temporary, &bytes)?;
        replace_file(&temporary, self.path.as_ref())
    }

    fn guard(&self) -> LumoResult<MutexGuard<'_, ()>> {
        self.lock
            .lock()
            .map_err(|_| LumoError::Storage("pending onboarding lock poisoned".to_owned()))
    }
}

fn pending_conflict() -> LumoError {
    LumoError::InvalidInput(
        "another onboarding recovery is pending; retry the original flow".to_owned(),
    )
}

fn write_private(path: &Path, bytes: &[u8]) -> LumoResult<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(storage_error)?;
    file.write_all(bytes).map_err(storage_error)?;
    file.sync_all().map_err(storage_error)
}

#[cfg(target_os = "android")]
fn replace_file(temporary: &Path, destination: &Path) -> LumoResult<()> {
    // Android app sandboxes are private, but some OEM filesystems/SELinux policies reject hard
    // links with EACCES. The lifecycle mutex guarantees that only this process can install an
    // onboarding marker, so a same-directory rename stays atomic without hard-link support.
    if destination.exists() {
        let _ = fs::remove_file(temporary);
        return Err(pending_conflict());
    }
    if let Err(error) = fs::rename(temporary, destination) {
        let _ = fs::remove_file(temporary);
        return Err(storage_error(error));
    }
    Ok(())
}

#[cfg(not(target_os = "android"))]
fn replace_file(temporary: &Path, destination: &Path) -> LumoResult<()> {
    if let Err(error) = fs::hard_link(temporary, destination) {
        let _ = fs::remove_file(temporary);
        return Err(storage_error(error));
    }
    fs::remove_file(temporary).map_err(storage_error)?;
    #[cfg(unix)]
    {
        let parent = destination.parent().ok_or_else(|| {
            LumoError::Storage("pending onboarding path has no parent".to_owned())
        })?;
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(storage_error)?;
    }
    Ok(())
}

fn storage_error(error: impl fmt::Display) -> LumoError {
    LumoError::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_id_survives_restart_and_double_invoke_until_confirmed() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("pending-onboarding.json");
        let first = PendingOnboardingStore::new(&path);
        let request_id = first.begin_create().expect("begin create");
        assert_eq!(first.begin_create().expect("double invoke"), request_id);

        let restarted = PendingOnboardingStore::new(&path);
        assert_eq!(restarted.begin_create().expect("restart"), request_id);
        let json: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).expect("pending file")).expect("json");
        assert_eq!(json.as_object().expect("object").len(), 2);
        assert!(json.get("requestId").is_some());
        assert!(json.get("pin").is_none());
        assert!(json.get("token").is_none());

        restarted.confirm_onboarding().expect("confirm");
        assert_ne!(restarted.begin_create().expect("new request"), request_id);
    }

    #[test]
    fn join_recovery_is_bound_to_invitation_without_storing_token_or_pin() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("pending-onboarding.json");
        let store = PendingOnboardingStore::new(&path);
        let invitation_id = Uuid::new_v4().to_string();
        let request_id = store.begin_join(&invitation_id).expect("begin join");
        assert_eq!(store.begin_join(&invitation_id).expect("retry"), request_id);
        assert!(matches!(
            store.begin_join(&Uuid::new_v4().to_string()),
            Err(LumoError::InvalidInput(_))
        ));
        let text = fs::read_to_string(path).expect("pending file");
        assert!(text.contains(&invitation_id));
        assert!(!text.contains("token"));
        assert!(!text.contains("pin"));
    }

    #[test]
    fn leave_marker_survives_restart_until_vault_cleanup_is_confirmed() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("pending-onboarding.json");
        let store = PendingOnboardingStore::new(&path);
        let request_id = store.begin_leave().expect("mark leave");
        assert_eq!(store.begin_leave().expect("idempotent leave"), request_id);

        let restarted = PendingOnboardingStore::new(&path);
        assert!(restarted.is_leave_pending().expect("leave marker"));
        restarted.confirm_onboarding().expect("clear marker");
        assert!(!restarted.is_leave_pending().expect("cleared marker"));
    }
}
