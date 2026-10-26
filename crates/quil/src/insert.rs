use std::{ptr::NonNull, thread, time::Duration};

use enigo::{Enigo, Keyboard, Settings};
use macos_accessibility_client::accessibility;
use objc2_application_services::{AXError, AXUIElement};
use objc2_core_foundation::{CFRetained, CFString, CFType};
use thiserror::Error;

const SELECTED_TEXT_ATTRIBUTE: &str = "AXSelectedText";
const FOCUSED_ELEMENT_ATTRIBUTE: &str = "AXFocusedUIElement";

#[derive(Debug, Error)]
pub enum InsertError {
    #[error(
        "Accessibility permission was not granted. Enable it for your terminal or host app in System Settings → Privacy & Security → Accessibility, then retry"
    )]
    Permission,
    #[error("text insertion failed: {0}")]
    Input(String),
}

pub fn require_accessibility() -> Result<(), InsertError> {
    if accessibility::application_is_trusted() {
        return Ok(());
    }
    eprintln!(
        "Quil needs Accessibility access only to insert dictated text. Enable it for your terminal or host app in System Settings → Privacy & Security → Accessibility. Waiting 30s…"
    );
    let _ = accessibility::application_is_trusted_with_prompt();
    for _ in 0..60 {
        if accessibility::application_is_trusted() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(500));
    }
    Err(InsertError::Permission)
}

pub struct TextInserter {
    enigo: Enigo,
}

impl TextInserter {
    pub fn new() -> Result<Self, InsertError> {
        let enigo = Enigo::new(&Settings::default())
            .map_err(|error| InsertError::Input(error.to_string()))?;
        Ok(Self { enigo })
    }

    /// Prefer AXSelectedText, which inserts at the current cursor/selection,
    /// then use Core Graphics keyboard events for terminal/editor controls that
    /// do not expose a settable selected-text attribute.
    pub fn insert(&mut self, text: &str) -> Result<(), InsertError> {
        if text.is_empty() {
            return Ok(());
        }
        if insert_selected_text(text) {
            return Ok(());
        }
        self.enigo
            .text(text)
            .map_err(|error| InsertError::Input(error.to_string()))
    }
}

fn focused_element() -> Option<CFRetained<AXUIElement>> {
    // SAFETY: the system-wide AXUIElement constructor returns a retained,
    // correctly typed object. CFRetained handles its lifetime.
    let system = unsafe { AXUIElement::new_system_wide() };
    let attribute = CFString::from_str(FOCUSED_ELEMENT_ATTRIBUTE);
    let mut raw: *const CFType = std::ptr::null();
    // SAFETY: `attribute` is a valid CFString and `raw` points to writable
    // storage for one CFTypeRef output value.
    let error = unsafe { system.copy_attribute_value(&attribute, NonNull::new(&mut raw)?) };
    if error != AXError::Success || raw.is_null() {
        return None;
    }
    let raw = NonNull::new(raw as *mut CFType)?;
    // SAFETY: AXUIElementCopyAttributeValue follows the Create rule and returns
    // a retained CFTypeRef. It is non-null and remains valid while downcast.
    let value = unsafe { CFRetained::from_raw(raw) };
    value.downcast::<AXUIElement>().ok()
}

fn insert_selected_text(text: &str) -> bool {
    let Some(focused) = focused_element() else {
        return false;
    };
    let attribute = CFString::from_str(SELECTED_TEXT_ATTRIBUTE);
    let mut settable = 0_u8;
    // SAFETY: `focused` and `attribute` are valid, and `settable` points to a
    // writable Boolean output byte.
    let error = unsafe { focused.is_attribute_settable(&attribute, NonNull::from(&mut settable)) };
    if error != AXError::Success || settable == 0 {
        return false;
    }
    let value = CFString::from_str(text);
    // SAFETY: the selected-text attribute was confirmed settable, and `value`
    // is a valid immutable CFString for the duration of the call.
    (unsafe { focused.set_attribute_value(&attribute, &value) }) == AXError::Success
}
