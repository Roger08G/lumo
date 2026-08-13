use std::{fmt, str::FromStr};

use lumo_core::{
    application::{CreateGroupInput, CreatePlaceInput, ReportLocationInput, SetTrackingInput},
    domain::{Connectivity, PermissionState, PlaceIcon, PlaceKind, PlaceTone, RuntimeState},
    LumoError, LumoResult, LumoService,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SimulationScenario {
    Home,
    Supermarket,
    Medical,
    Away,
    Offline,
    Permission,
    Battery,
    Help,
}

impl fmt::Display for SimulationScenario {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}",
            serde_json::to_value(self)
                .unwrap_or_default()
                .as_str()
                .unwrap_or("unknown")
        )
    }
}

impl FromStr for SimulationScenario {
    type Err = LumoError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "home" => Ok(Self::Home),
            "supermarket" => Ok(Self::Supermarket),
            "medical" => Ok(Self::Medical),
            "away" => Ok(Self::Away),
            "offline" => Ok(Self::Offline),
            "permission" => Ok(Self::Permission),
            "battery" => Ok(Self::Battery),
            "help" => Ok(Self::Help),
            _ => Err(LumoError::InvalidInput(format!(
                "unknown scenario: {value}"
            ))),
        }
    }
}

pub fn seed_demo(
    service: &LumoService,
    state: &mut RuntimeState,
    pin: &str,
    now_ms: i64,
) -> LumoResult<()> {
    service.create_group(
        state,
        CreateGroupInput {
            name: "Grupo familiar".into(),
            supervisor_name: "Supervisor".into(),
            supervisor_phone: "+34600000001".into(),
            tracked_person_name: "Persona acompañada".into(),
            tracked_person_phone: "+34600000002".into(),
            pin: pin.into(),
        },
        now_ms,
    )?;
    for place in default_places() {
        service.create_place(state, place, now_ms)?;
    }
    service.set_tracking(
        state,
        SetTrackingInput {
            precise_permission: PermissionState::Granted,
            background_permission: PermissionState::Granted,
            battery_optimization_disabled: true,
            enabled: true,
        },
        now_ms,
    )?;
    service.set_connectivity(state, Connectivity::Online, now_ms)?;
    Ok(())
}

pub fn apply_scenario(
    service: &LumoService,
    state: &mut RuntimeState,
    scenario: SimulationScenario,
    now_ms: i64,
) -> LumoResult<()> {
    match scenario {
        SimulationScenario::Home => report(service, state, 40.4168, -3.7038, 68, now_ms),
        SimulationScenario::Supermarket => report(service, state, 40.4191, -3.7072, 65, now_ms),
        SimulationScenario::Medical => report(service, state, 40.4154, -3.7061, 61, now_ms),
        SimulationScenario::Away => report(service, state, 40.4310, -3.7200, 58, now_ms),
        SimulationScenario::Offline => {
            service.set_connectivity(state, Connectivity::Offline, now_ms)
        }
        SimulationScenario::Permission => service.set_tracking(
            state,
            SetTrackingInput {
                precise_permission: PermissionState::Revoked,
                background_permission: PermissionState::Revoked,
                battery_optimization_disabled: false,
                enabled: false,
            },
            now_ms,
        ),
        SimulationScenario::Battery => {
            if !state.controlled.tracking_enabled {
                service.set_tracking(
                    state,
                    SetTrackingInput {
                        precise_permission: PermissionState::Granted,
                        background_permission: PermissionState::Granted,
                        battery_optimization_disabled: true,
                        enabled: true,
                    },
                    now_ms,
                )?;
            }
            let (latitude, longitude) = state
                .controlled
                .last_location
                .as_ref()
                .map(|sample| (sample.latitude, sample.longitude))
                .unwrap_or((40.4168, -3.7038));
            report(service, state, latitude, longitude, 12, now_ms)
        }
        SimulationScenario::Help => service.send_help(state, now_ms),
    }
}

fn report(
    service: &LumoService,
    state: &mut RuntimeState,
    latitude: f64,
    longitude: f64,
    battery_percent: u8,
    now_ms: i64,
) -> LumoResult<()> {
    if !state.controlled.tracking_enabled {
        service.set_tracking(
            state,
            SetTrackingInput {
                precise_permission: PermissionState::Granted,
                background_permission: PermissionState::Granted,
                battery_optimization_disabled: true,
                enabled: true,
            },
            now_ms,
        )?;
    }
    service.report_location(
        state,
        ReportLocationInput {
            latitude,
            longitude,
            accuracy_m: 8.0,
            battery_percent,
            captured_at_ms: Some(now_ms),
        },
        now_ms,
    )
}

fn default_places() -> [CreatePlaceInput; 3] {
    [
        CreatePlaceInput {
            name: "Casa".into(),
            address: "Dirección principal".into(),
            latitude: 40.4168,
            longitude: -3.7038,
            radius_m: 50,
            kind: PlaceKind::Home,
            color: PlaceTone::Purple,
            icon: PlaceIcon::Home,
        },
        CreatePlaceInput {
            name: "Supermercado".into(),
            address: "Dirección habitual".into(),
            latitude: 40.4191,
            longitude: -3.7072,
            radius_m: 50,
            kind: PlaceKind::Shop,
            color: PlaceTone::Yellow,
            icon: PlaceIcon::Shopping,
        },
        CreatePlaceInput {
            name: "Centro médico".into(),
            address: "Dirección sanitaria".into(),
            latitude: 40.4154,
            longitude: -3.7061,
            radius_m: 50,
            kind: PlaceKind::Medical,
            color: PlaceTone::Pink,
            icon: PlaceIcon::Health,
        },
    ]
}
