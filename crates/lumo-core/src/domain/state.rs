use serde::{Deserialize, Serialize};

use super::{
    ControlledDevice, Group, GroupRole, Invitation, PendingCommand, PinGuard, Place, TimelineEvent,
};

pub const SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeProfile {
    Controller,
    Controlled,
    Debug,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeState {
    pub schema_version: u32,
    pub revision: u64,
    pub group: Option<Group>,
    pub controlled: ControlledDevice,
    pub places: Vec<Place>,
    pub events: Vec<TimelineEvent>,
    pub commands: Vec<PendingCommand>,
    pub invitations: Vec<Invitation>,
    pub pin_guard: PinGuard,
    pub next_sequence: u64,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            revision: 0,
            group: None,
            controlled: ControlledDevice::default(),
            places: Vec::new(),
            events: Vec::new(),
            commands: Vec::new(),
            invitations: Vec::new(),
            pin_guard: PinGuard::default(),
            next_sequence: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    pub group_id: String,
    pub group_name: String,
    pub group_code: String,
    pub supervisor_name: String,
    pub supervisor_phone: String,
    pub tracked_person_name: String,
    pub tracked_person_phone: String,
    pub role: GroupRole,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub schema_version: u32,
    pub revision: u64,
    pub profile: RuntimeProfile,
    pub session: Option<SessionView>,
    pub controlled: ControlledDevice,
    pub places: Vec<Place>,
    pub events: Vec<TimelineEvent>,
    pub commands: Vec<PendingCommand>,
}

impl RuntimeState {
    pub fn snapshot(&self, profile: RuntimeProfile) -> AppSnapshot {
        AppSnapshot {
            schema_version: self.schema_version,
            revision: self.revision,
            profile,
            session: self.group.as_ref().map(|group| SessionView {
                group_id: group.id.clone(),
                group_name: group.name.clone(),
                group_code: group.code.clone(),
                supervisor_name: group.supervisor_name.clone(),
                supervisor_phone: group.supervisor_phone.clone(),
                tracked_person_name: group.tracked_person_name.clone(),
                tracked_person_phone: group.tracked_person_phone.clone(),
                role: match profile {
                    RuntimeProfile::Controlled => GroupRole::Member,
                    RuntimeProfile::Controller | RuntimeProfile::Debug => GroupRole::Supervisor,
                },
            }),
            controlled: self.controlled.clone(),
            places: self.places.clone(),
            events: self.events.clone(),
            commands: self.commands.clone(),
        }
    }
}
