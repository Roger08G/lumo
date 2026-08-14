use std::sync::Arc;

use lumo_core::{
    application::{
        CreateGroupInput, CreatePlaceInput, InvitationView, ReportLocationInput, SetTrackingInput,
    },
    domain::{AppSnapshot, Connectivity, Place, RuntimeProfile, RuntimeState},
    ports::{Clock, StateRepository},
    LumoError, LumoResult, LumoService,
};
use lumo_protocol::ControlledOperation;

use crate::{
    simulation::{apply_scenario, seed_demo, SimulationScenario},
    storage::ControlledOperationPort,
};

const MAX_REVISION_ATTEMPTS: usize = 4;

#[derive(Clone)]
pub struct LocalBackend<R> {
    repository: Arc<R>,
    clock: Arc<dyn Clock>,
    service: LumoService,
}

impl<R> std::fmt::Debug for LocalBackend<R> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalBackend")
            .field("repository", &std::any::type_name::<R>())
            .field("clock", &"Clock")
            .finish()
    }
}

impl<R: StateRepository + ControlledOperationPort + 'static> LocalBackend<R> {
    pub fn new(repository: R, clock: impl Clock + 'static) -> Self {
        Self {
            repository: Arc::new(repository),
            clock: Arc::new(clock),
            service: LumoService,
        }
    }

    pub fn with_clock(repository: R, clock: Arc<dyn Clock>) -> Self {
        Self {
            repository: Arc::new(repository),
            clock,
            service: LumoService,
        }
    }

    fn transact_with_revision_retry<T, F>(&self, mut operation: F) -> LumoResult<T>
    where
        F: FnMut(&mut RuntimeState) -> LumoResult<T>,
    {
        for attempt in 0..MAX_REVISION_ATTEMPTS {
            match self.repository.transact(|state| operation(state)) {
                Err(LumoError::RevisionConflict) if attempt + 1 < MAX_REVISION_ATTEMPTS => {}
                result => return result,
            }
        }
        Err(LumoError::RevisionConflict)
    }

    pub fn snapshot(&self, profile: RuntimeProfile) -> LumoResult<AppSnapshot> {
        if profile == RuntimeProfile::Controlled {
            if let Some(snapshot) = self.repository.load_controlled_snapshot()? {
                return Ok(snapshot);
            }
        }
        let now_ms = self.clock.now_ms();
        let mut state = self.repository.load()?;
        Ok(self.service.snapshot(&mut state, profile, now_ms))
    }

    pub fn create_group(
        &self,
        input: CreateGroupInput,
        profile: RuntimeProfile,
    ) -> LumoResult<AppSnapshot> {
        let now_ms = self.clock.now_ms();
        self.transact_with_revision_retry(|state| {
            self.service.create_group(state, input.clone(), now_ms)?;
            Ok(self.service.snapshot(state, profile, now_ms))
        })
    }

    pub fn verify_pin(&self, pin: &str) -> LumoResult<()> {
        let now_ms = self.clock.now_ms();
        self.transact_with_revision_retry(|state| {
            self.service.verify_protected_action(state, pin, now_ms)
        })
    }

    pub fn create_invitation(&self, pin: &str) -> LumoResult<InvitationView> {
        let now_ms = self.clock.now_ms();
        self.transact_with_revision_retry(|state| {
            self.service.create_invitation(state, pin, now_ms)
        })
    }

    pub fn leave_group(&self, pin: &str) -> LumoResult<()> {
        let now_ms = self.clock.now_ms();
        self.transact_with_revision_retry(|state| self.service.leave_group(state, pin, now_ms))
    }

    pub fn consume_invitation(&self, token: &str, pin: &str) -> LumoResult<()> {
        let now_ms = self.clock.now_ms();
        self.transact_with_revision_retry(|state| {
            self.service.consume_invitation(state, token, pin, now_ms)
        })
    }

    pub fn create_place(&self, input: CreatePlaceInput) -> LumoResult<Place> {
        let now_ms = self.clock.now_ms();
        self.transact_with_revision_retry(|state| {
            self.service.create_place(state, input.clone(), now_ms)
        })
    }

    pub fn update_place(&self, id: &str, input: CreatePlaceInput) -> LumoResult<Place> {
        let now_ms = self.clock.now_ms();
        self.transact_with_revision_retry(|state| {
            self.service.update_place(state, id, input.clone(), now_ms)
        })
    }

    pub fn delete_place(&self, id: &str, pin: &str) -> LumoResult<AppSnapshot> {
        let now_ms = self.clock.now_ms();
        self.transact_with_revision_retry(|state| {
            self.service.delete_place(state, id, pin, now_ms)?;
            Ok(self
                .service
                .snapshot(state, RuntimeProfile::Controller, now_ms))
        })
    }

    pub fn set_tracking(&self, input: SetTrackingInput) -> LumoResult<AppSnapshot> {
        if let Some(response) = self
            .repository
            .apply_controlled_operation(ControlledOperation::SetTracking(input.clone()))?
        {
            return Ok(response.snapshot);
        }
        let now_ms = self.clock.now_ms();
        self.transact_with_revision_retry(|state| {
            self.service.set_tracking(state, input.clone(), now_ms)?;
            Ok(self
                .service
                .snapshot(state, RuntimeProfile::Controlled, now_ms))
        })
    }

    pub fn set_connectivity(&self, connectivity: Connectivity) -> LumoResult<AppSnapshot> {
        if let Some(response) = self
            .repository
            .apply_controlled_operation(ControlledOperation::SetConnectivity { connectivity })?
        {
            return Ok(response.snapshot);
        }
        let now_ms = self.clock.now_ms();
        self.transact_with_revision_retry(|state| {
            self.service.set_connectivity(state, connectivity, now_ms)?;
            Ok(self
                .service
                .snapshot(state, RuntimeProfile::Controlled, now_ms))
        })
    }

    pub fn report_location(&self, input: ReportLocationInput) -> LumoResult<AppSnapshot> {
        if let Some(response) = self
            .repository
            .apply_controlled_operation(ControlledOperation::ReportLocation(input.clone()))?
        {
            return Ok(response.snapshot);
        }
        let now_ms = self.clock.now_ms();
        self.transact_with_revision_retry(|state| {
            self.service.report_location(state, input.clone(), now_ms)?;
            Ok(self
                .service
                .snapshot(state, RuntimeProfile::Controlled, now_ms))
        })
    }

    pub fn request_location(&self) -> LumoResult<String> {
        let now_ms = self.clock.now_ms();
        self.transact_with_revision_retry(|state| self.service.request_location(state, now_ms))
    }

    pub fn process_pending(&self) -> LumoResult<usize> {
        if let Some(response) = self
            .repository
            .apply_controlled_operation(ControlledOperation::ProcessPending)?
        {
            return response.processed.ok_or_else(|| {
                LumoError::Serialization(
                    "member operation response omitted the processed count".to_owned(),
                )
            });
        }
        let now_ms = self.clock.now_ms();
        self.transact_with_revision_retry(|state| self.service.process_pending(state, now_ms))
    }

    pub fn send_help(&self) -> LumoResult<AppSnapshot> {
        if let Some(response) = self
            .repository
            .apply_controlled_operation(ControlledOperation::SendHelp)?
        {
            return Ok(response.snapshot);
        }
        let now_ms = self.clock.now_ms();
        self.transact_with_revision_retry(|state| {
            self.service.send_help(state, now_ms)?;
            Ok(self
                .service
                .snapshot(state, RuntimeProfile::Controlled, now_ms))
        })
    }

    pub fn mark_events_read(&self) -> LumoResult<AppSnapshot> {
        let now_ms = self.clock.now_ms();
        self.transact_with_revision_retry(|state| {
            self.service.mark_events_read(state, now_ms)?;
            Ok(self
                .service
                .snapshot(state, RuntimeProfile::Controller, now_ms))
        })
    }

    pub fn debug_seed(&self, pin: &str) -> LumoResult<AppSnapshot> {
        let now_ms = self.clock.now_ms();
        self.transact_with_revision_retry(|state| {
            seed_demo(&self.service, state, pin, now_ms)?;
            Ok(self.service.snapshot(state, RuntimeProfile::Debug, now_ms))
        })
    }

    pub fn debug_scenario(&self, scenario: SimulationScenario) -> LumoResult<AppSnapshot> {
        let now_ms = self.clock.now_ms();
        self.transact_with_revision_retry(|state| {
            apply_scenario(&self.service, state, scenario, now_ms)?;
            Ok(self.service.snapshot(state, RuntimeProfile::Debug, now_ms))
        })
    }

    pub fn reset(&self) -> LumoResult<()> {
        self.transact_with_revision_retry(|state| {
            self.service.reset(state);
            Ok(())
        })
    }
}
