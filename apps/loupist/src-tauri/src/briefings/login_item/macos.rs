use super::{installed_bundle, system_status, LoginItemStatus, Service};
use objc2::{class, msg_send, rc::Retained, runtime::AnyObject};
use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager, NSBundle, NSError};

#[link(name = "ServiceManagement", kind = "framework")]
extern "C" {}

pub(crate) fn available() -> bool {
    if cfg!(any(
        not(loupist_release_build),
        dev,
        test,
        debug_assertions,
        feature = "webdriver",
        feature = "e2e"
    )) {
        return false;
    }
    let bundle = NSBundle::mainBundle();
    let Some(id) = bundle.bundleIdentifier() else {
        return false;
    };
    std::fs::canonicalize(bundle.bundlePath().to_string())
        .is_ok_and(|path| installed_bundle(&id.to_string(), &path))
}

pub(crate) fn login_launch() -> bool {
    // Called synchronously during Tao's applicationDidFinishLaunching -> Tauri setup.
    let Some(event) = NSAppleEventManager::sharedAppleEventManager().currentAppleEvent() else {
        return false;
    };
    // NSAppleEventDescriptor's event ID, AEKeyword and enum code are four-byte OSTypes.
    unsafe {
        let id: u32 = msg_send![&event, eventID];
        let property: Option<Retained<NSAppleEventDescriptor>> =
            msg_send![&event, paramDescriptorForKeyword: u32::from_be_bytes(*b"prdt")];
        is_login_launch(id, property.map(|value| value.enumCodeValue()))
    }
}

fn is_login_launch(event_id: u32, property: Option<u32>) -> bool {
    // kAEOpenApplication with keyAEPropData == keyAELaunchedAsLogInItem.
    event_id == u32::from_be_bytes(*b"oapp") && property == Some(u32::from_be_bytes(*b"lgit"))
}

#[cfg(test)]
#[test]
fn only_the_login_open_application_event_starts_hidden() {
    let open = u32::from_be_bytes(*b"oapp");
    let login = Some(u32::from_be_bytes(*b"lgit"));
    assert!(is_login_launch(open, login));
    assert!(!is_login_launch(open, None));
    assert!(!is_login_launch(open, Some(u32::from_be_bytes(*b"serv"))));
    assert!(!is_login_launch(u32::from_be_bytes(*b"rapp"), login));
}

pub(super) struct MainApp;
impl MainApp {
    fn service(&self) -> Retained<AnyObject> {
        // The deployment target is macOS 13. mainAppService returns an SMAppService.
        unsafe { msg_send![class!(SMAppService), mainAppService] }
    }
}

impl Service for MainApp {
    fn status(&self) -> LoginItemStatus {
        system_status(unsafe { msg_send![&*self.service(), status] })
    }

    fn set(&self, enabled: bool) -> LoginItemStatus {
        // Both methods return BOOL and allow a null NSError out parameter.
        let error = std::ptr::null_mut::<*mut NSError>();
        let ok: bool = unsafe {
            if enabled {
                msg_send![&*self.service(), registerAndReturnError: error]
            } else {
                msg_send![&*self.service(), unregisterAndReturnError: error]
            }
        };
        let status = self.status();
        if ok
            || (enabled
                && matches!(
                    status,
                    LoginItemStatus::On | LoginItemStatus::RequiresApproval
                ))
            || (!enabled && status == LoginItemStatus::Off)
        {
            status
        } else {
            LoginItemStatus::Error
        }
    }

    fn open_settings(&self) {
        unsafe {
            let _: () = msg_send![class!(SMAppService), openSystemSettingsLoginItems];
        }
    }
}
