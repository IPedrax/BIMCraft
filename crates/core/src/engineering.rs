//! Engineering systems and Engineering Level Mode: which elements belong to a low-voltage or building-services system
//! (electrical, data, low-voltage/SS, plumbing), and the colour each system's routes draw in on a plan that shows them whole.

use crate::family::Category;
use crate::model::{Element, Model};
use serde::{Deserialize, Serialize};

/// An engineering system a plan draws apart from the architecture. Its name on a file is kebab-case.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum System {
    Electrical,
    Data,
    #[default]
    LowVoltage,
    Plumbing,
}

impl System {
    /// The colour its routes draw in: thick lines, by system.
    pub fn color(self) -> [u8; 3] {
        match self {
            System::Electrical => [220, 90, 0],
            System::Data => [0, 140, 70],
            System::LowVoltage => [150, 40, 160],
            System::Plumbing => [0, 110, 200],
        }
    }
}

/// The system a family category belongs to, when it is an engineering one.
pub fn system_of_category(category: Category) -> Option<System> {
    match category {
        Category::Plumbing | Category::Drain | Category::PipeFitting | Category::PipeAccessory => Some(System::Plumbing),
        Category::Equipment
        | Category::Outlet
        | Category::Switch
        | Category::AirTerminal
        | Category::Earthing
        | Category::InspectionBox
        | Category::Lighting => Some(System::Electrical),
        Category::DataOutlet | Category::VoiceOutlet | Category::PatchPanel | Category::Rack => Some(System::Data),
        Category::AlarmPanel
        | Category::SmokeDetector
        | Category::HeatDetector
        | Category::CallPoint
        | Category::Sounder
        | Category::ExitSign
        | Category::Speaker
        | Category::MotionDetector
        | Category::Camera => Some(System::LowVoltage),
        _ => None,
    }
}

/// The engineering system a run's `system` label names, if it names one.
fn system_of_text(system: &str) -> Option<System> {
    match system.to_lowercase().as_str() {
        "electrical" => Some(System::Electrical),
        "data" => Some(System::Data),
        "low-voltage" | "lowvoltage" | "ss" => Some(System::LowVoltage),
        "plumbing" => Some(System::Plumbing),
        _ => None,
    }
}

/// The system `e` belongs to, if it is an engineering element on that level; a wired or carrier run belongs to the system
/// its label names, and otherwise to its default.
pub fn system_of(m: &Model, e: &Element) -> Option<System> {
    match e {
        Element::Pipe(_) => Some(System::Plumbing),
        Element::Cable(_) | Element::DownConductor(_) | Element::GroundConductor(_) => Some(System::Electrical),
        Element::CableCarrier(c) => system_of_text(&c.system).or(Some(System::Electrical)),
        Element::Wire(w) => system_of_text(&w.system).or(Some(System::LowVoltage)),
        Element::Component(c) => m.families.get(&c.family).and_then(|f| system_of_category(f.category)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engineering_systems_cover_their_categories() {
        assert_eq!(system_of_category(Category::Plumbing), Some(System::Plumbing));
        assert_eq!(system_of_category(Category::Lighting), Some(System::Electrical));
        assert_eq!(system_of_category(Category::Rack), Some(System::Data));
        assert_eq!(system_of_category(Category::SmokeDetector), Some(System::LowVoltage));
        assert_eq!(system_of_category(Category::Furniture), None);
        assert_ne!(System::Plumbing.color(), System::LowVoltage.color());
    }

    #[test]
    fn a_system_names_itself_kebab_case_on_a_file_and_defaults_to_low_voltage() {
        assert_eq!(serde_json::to_value(System::LowVoltage).unwrap(), serde_json::json!("low-voltage"));
        assert_eq!(serde_json::from_value::<System>(serde_json::json!("plumbing")).unwrap(), System::Plumbing);
        assert_eq!(System::LowVoltage, System::default());
    }
}
