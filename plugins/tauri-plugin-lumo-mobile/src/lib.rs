mod error;
mod mobile;
mod models;
#[cfg(test)]
mod queue_policy;

pub use error::{Error, Result};
pub use mobile::LumoMobile;
pub use models::{DeviceCredential, MobileStatus};

use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Manager, Runtime};

pub trait LumoMobileExt<R: Runtime> {
    fn lumo_mobile(&self) -> &LumoMobile<R>;
}

impl<R: Runtime, T: Manager<R>> LumoMobileExt<R> for T {
    fn lumo_mobile(&self) -> &LumoMobile<R> {
        self.state::<LumoMobile<R>>().inner()
    }
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("lumo-mobile-native")
        .setup(|app, api| {
            let mobile = mobile::init(app, api)?;
            app.manage(mobile);
            Ok(())
        })
        .build()
}
