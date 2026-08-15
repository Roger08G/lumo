#[cfg(not(target_os = "android"))]
use std::marker::PhantomData;

use serde::{de::DeserializeOwned, Serialize};
use tauri::plugin::PluginApi;
#[cfg(target_os = "android")]
use tauri::plugin::PluginHandle;
use tauri::{AppHandle, Runtime};

use crate::models::{
    AddressResponse, CoordinatesPayload, CredentialLoadResponse, DeviceCredential, MobileStatus,
    NotificationPayload, PendingAlarm, PendingAlarmResponse, PhonePayload, RolePayload,
    TrackingPayload,
};
#[cfg(not(target_os = "android"))]
use crate::Error;
use crate::Result;

#[cfg(target_os = "android")]
const PLUGIN_IDENTIFIER: &str = "app.lumo.family.mobile";

pub(crate) fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> Result<LumoMobile<R>> {
    #[cfg(target_os = "android")]
    let bridge = LumoMobile {
        handle: api.register_android_plugin(PLUGIN_IDENTIFIER, "LumoMobilePlugin")?,
    };
    #[cfg(not(target_os = "android"))]
    let bridge = {
        let _ = api;
        LumoMobile {
            marker: PhantomData,
        }
    };
    Ok(bridge)
}

pub struct LumoMobile<R: Runtime> {
    #[cfg(target_os = "android")]
    handle: PluginHandle<R>,
    #[cfg(not(target_os = "android"))]
    marker: PhantomData<fn() -> R>,
}

impl<R: Runtime> LumoMobile<R> {
    fn run<T: DeserializeOwned>(&self, command: &str, payload: impl Serialize) -> Result<T> {
        #[cfg(target_os = "android")]
        {
            self.handle
                .run_mobile_plugin(command, payload)
                .map_err(Into::into)
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = (command, payload);
            Err(Error::UnsupportedPlatform)
        }
    }

    pub fn get_status(&self) -> Result<MobileStatus> {
        self.run("getStatus", ())
    }

    pub fn store_credential(&self, credential: &DeviceCredential) -> Result<()> {
        self.run("storeCredential", credential)
    }

    pub fn load_credential(&self) -> Result<Option<DeviceCredential>> {
        #[cfg(target_os = "android")]
        {
            self.run::<CredentialLoadResponse>("loadCredential", ())
                .map(|response| response.credential)
                // Tauri includes malformed response JSON in its low-level error. Never allow a
                // credential-bearing response to escape through Display, Debug, JS, or logs.
                .map_err(|_| crate::Error::CredentialBridge)
        }
        #[cfg(not(target_os = "android"))]
        {
            self.run::<CredentialLoadResponse>("loadCredential", ())
                .map(|response| response.credential)
        }
    }

    pub fn clear_credential(&self) -> Result<()> {
        self.run("clearCredential", ())
    }

    pub fn request_permissions(&self, role: &str) -> Result<MobileStatus> {
        self.run("requestPermissions", RolePayload { role })
    }

    pub fn configure_tracking(
        &self,
        role: &str,
        enabled: bool,
        interval_seconds: u64,
    ) -> Result<MobileStatus> {
        self.run(
            "configureTracking",
            TrackingPayload {
                role,
                enabled,
                interval_seconds: interval_seconds.clamp(5, 900),
            },
        )
    }

    pub fn open_phone_dialer(&self, number: &str) -> Result<()> {
        self.run("openPhoneDialer", PhonePayload { number })
    }

    pub fn reverse_geocode(&self, latitude: f64, longitude: f64) -> Result<Option<String>> {
        self.run::<AddressResponse>(
            "reverseGeocode",
            CoordinatesPayload {
                latitude,
                longitude,
            },
        )
        .map(|response| response.address)
    }

    pub fn show_notification(
        &self,
        id: Option<&str>,
        title: &str,
        body: &str,
        urgent: bool,
    ) -> Result<()> {
        self.run(
            "showNotification",
            NotificationPayload {
                id,
                title,
                body,
                urgent,
            },
        )
    }

    pub fn start_emergency_alarm(&self, alarm: &PendingAlarm) -> Result<()> {
        self.run("startEmergencyAlarm", alarm)
    }

    pub fn pending_alarm(&self) -> Result<Option<PendingAlarm>> {
        self.run::<PendingAlarmResponse>("getPendingAlarm", ())
            .map(|response| response.alarm)
    }

    pub fn stop_emergency_alarm(&self) -> Result<()> {
        self.run("stopEmergencyAlarm", ())
    }

    pub fn open_battery_settings(&self) -> Result<()> {
        self.run("openBatterySettings", ())
    }
}
