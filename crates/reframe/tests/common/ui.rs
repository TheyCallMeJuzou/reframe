//! What a screen reader sees of the app, through Windows' UI Automation: its elements by role and
//! name, their values and check marks, and clicks on them. The tests find and press things the
//! way the Electron version's tests did with Playwright's `getByRole`.

use std::time::Instant;

use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::*;

use super::WAIT;

/// An element as UI Automation shows it.
#[derive(Clone, Debug)]
pub struct Seen {
    pub role: &'static str,
    pub name: String,
    pub enabled: bool,
    /// A text field's or a choice box's value.
    pub value: Option<String>,
    /// A check box's mark.
    pub checked: Option<bool>,
}

/// The app's windows, read through UI Automation.
pub struct Ui {
    automation: IUIAutomation,
    pid: u32,
}

#[allow(non_upper_case_globals)] // the names Windows gives them
fn role_of(control: UIA_CONTROLTYPE_ID) -> &'static str {
    match control {
        UIA_ButtonControlTypeId => "button",
        UIA_CheckBoxControlTypeId => "checkbox",
        UIA_ComboBoxControlTypeId => "combobox",
        UIA_EditControlTypeId => "edit",
        UIA_HyperlinkControlTypeId => "link",
        UIA_TextControlTypeId => "text",
        UIA_GroupControlTypeId => "group",
        UIA_ListItemControlTypeId => "listitem",
        UIA_ListControlTypeId => "list",
        UIA_DocumentControlTypeId => "document",
        UIA_PaneControlTypeId => "pane",
        UIA_WindowControlTypeId => "window",
        UIA_MenuItemControlTypeId => "menuitem",
        UIA_MenuControlTypeId => "menu",
        _ => "other",
    }
}

impl Ui {
    pub fn new(pid: u32) -> Ui {
        // SAFETY: COM is set up for this thread and the automation object made as documented.
        let automation = unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).expect("UI Automation")
        };
        Ui { automation, pid }
    }

    fn elements(&self) -> Vec<IUIAutomationElement> {
        let mut found = Vec::new();
        // SAFETY: plain UI Automation queries; every result is checked.
        unsafe {
            let Ok(root) = self.automation.GetRootElement() else {
                return found;
            };
            let Ok(of_app) = self
                .automation
                .CreatePropertyCondition(UIA_ProcessIdPropertyId, &VARIANT::from(self.pid as i32))
            else {
                return found;
            };
            let Ok(windows) = root.FindAll(TreeScope_Children, &of_app) else {
                return found;
            };
            let Ok(all) = self.automation.CreateTrueCondition() else {
                return found;
            };
            for index in 0..windows.Length().unwrap_or(0) {
                let Ok(window) = windows.GetElement(index) else {
                    continue;
                };
                let Ok(children) = window.FindAll(TreeScope_Children, &all) else {
                    continue;
                };
                for index in 0..children.Length().unwrap_or(0) {
                    let Ok(child) = children.GetElement(index) else {
                        continue;
                    };
                    // The title bar is Windows' own (its Close closes the window), not the app's.
                    if child.CurrentControlType().unwrap_or_default() == UIA_TitleBarControlTypeId {
                        continue;
                    }
                    found.push(child.clone());
                    let Ok(inside) = child.FindAll(TreeScope_Descendants, &all) else {
                        continue;
                    };
                    for index in 0..inside.Length().unwrap_or(0) {
                        if let Ok(element) = inside.GetElement(index) {
                            found.push(element);
                        }
                    }
                }
            }
        }
        found
    }

    fn describe(element: &IUIAutomationElement) -> Seen {
        // SAFETY: plain property reads; a pattern an element does not have is an error, taken as
        // "none".
        unsafe {
            let value = element
                .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                .and_then(|pattern| pattern.CurrentValue())
                .ok()
                .map(|value| value.to_string());
            let checked = element
                .GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId)
                .and_then(|pattern| pattern.CurrentToggleState())
                .ok()
                .map(|state| state == ToggleState_On);
            Seen {
                role: role_of(element.CurrentControlType().unwrap_or_default()),
                name: element.CurrentName().map(|name| name.to_string()).unwrap_or_default(),
                enabled: element.CurrentIsEnabled().is_ok_and(|enabled| enabled.as_bool()),
                value,
                checked,
            }
        }
    }

    /// Everything the app's windows show to a screen reader.
    pub fn all(&self) -> Vec<Seen> {
        self.elements().iter().map(Self::describe).collect()
    }

    /// The elements with this role.
    pub fn with_role(&self, role: &str) -> Vec<Seen> {
        self.all().into_iter().filter(|seen| seen.role == role).collect()
    }

    fn find_element(&self, role: &str, name: &str) -> Option<(IUIAutomationElement, Seen)> {
        self.elements().into_iter().find_map(|element| {
            let seen = Self::describe(&element);
            (seen.role == role && seen.name == name).then_some((element, seen))
        })
    }

    /// The element with this role and name, once it is there.
    pub fn wait(&self, role: &str, name: &str) -> Seen {
        self.wait_element(role, name).1
    }

    fn wait_element(&self, role: &str, name: &str) -> (IUIAutomationElement, Seen) {
        let deadline = Instant::now() + WAIT;
        loop {
            if let Some(found) = self.find_element(role, name) {
                return found;
            }
            assert!(
                Instant::now() < deadline,
                "no {role} {name:?} among {:#?}",
                self.all()
                    .into_iter()
                    .filter(|seen| !seen.name.is_empty())
                    .map(|seen| format!("{} {:?}", seen.role, seen.name))
                    .collect::<Vec<_>>()
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    /// Whether there is an element with this role and name right now.
    pub fn has(&self, role: &str, name: &str) -> bool {
        self.find_element(role, name).is_some()
    }

    /// Waits until no element with this role and name is left.
    pub fn wait_gone(&self, role: &str, name: &str) {
        let deadline = Instant::now() + WAIT;
        while self.has(role, name) {
            assert!(Instant::now() < deadline, "{role} {name:?} is still there");
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    /// Presses it (a button, a link) or turns it (a check box), as a click would.
    pub fn click(&self, role: &str, name: &str) {
        let (element, seen) = self.wait_element(role, name);
        assert!(seen.enabled, "{role} {name:?} is disabled");
        // SAFETY: plain pattern calls on an element that is there.
        unsafe {
            if let Ok(invoke) = element.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId) {
                invoke.Invoke().expect("click");
            } else if let Ok(toggle) = element.GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId) {
                toggle.Toggle().expect("toggle");
            } else if let Ok(expand) =
                element.GetCurrentPatternAs::<IUIAutomationExpandCollapsePattern>(UIA_ExpandCollapsePatternId)
            {
                if expand.CurrentExpandCollapseState() == Ok(ExpandCollapseState_Expanded) {
                    expand.Collapse().expect("collapse");
                } else {
                    expand.Expand().expect("expand");
                }
            } else {
                panic!("{role} {name:?} cannot be clicked");
            }
        }
    }

    /// Gives it the keyboard.
    pub fn focus(&self, role: &str, name: &str) {
        let (element, _) = self.wait_element(role, name);
        // SAFETY: plain call on an element that is there.
        unsafe { element.SetFocus().expect("focus") };
    }

    /// Waits until the element shows `value` (a text field, a choice box).
    pub fn wait_value(&self, role: &str, name: &str, value: &str) {
        let deadline = Instant::now() + WAIT;
        loop {
            let seen = self.wait(role, name);
            if seen.value.as_deref() == Some(value) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "{role} {name:?} shows {:?}, not {value:?}",
                seen.value
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    /// Waits until a check box is (not) checked.
    pub fn wait_checked(&self, name: &str, checked: bool) {
        let deadline = Instant::now() + WAIT;
        loop {
            if self.wait("checkbox", name).checked == Some(checked) {
                return;
            }
            assert!(Instant::now() < deadline, "check box {name:?} is not {checked}");
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}
