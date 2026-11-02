//! The discipline tabs after Structure: Plumbing, and the Electrical, Lightning Protection and Data tabs to come. A run (a
//! pipe, a conduit) is `Tool::Run` and a standing thing `Tool::Stand`, which the Structure tab built; what these tabs
//! place besides is a fixture or a device, one tool (`Tool::Fixture`) that places the families of the kinds it names. A tab adds
//! a ribbon panel and rows to the tables here, not a tool.

use bimcraft_i18n::msg;

use bimcraft_core::family::Category;
use egui::Ui;

use crate::icons::Icon;
use crate::structure::same_tool;
use crate::{BimApp, Tool, big_button, panel};

/// A fixture tool: the families of `kinds` are what it places, listed in its Type Selector, turned by Space.
pub(crate) struct FixtureTool {
    /// What `Tool::Fixture` names it by.
    pub key: &'static str,
    pub kinds: &'static [Category],
    /// Its two-letter shortcut, its button, its contextual tab's title and its tooltip.
    pub letters: &'static str,
    pub label: &'static str,
    pub title: &'static str,
    pub tip: &'static str,
    pub icon: Icon,
}

/// The run category a two-letter shortcut picks (the command's alias), if it is one.
pub(crate) fn run_with(letters: &str) -> Option<&'static str> {
    bimcraft_core::engine::find(letters).and_then(|c| bimcraft_core::run_category_of(c.id))
}

pub(crate) const FIXTURES: &[FixtureTool] = &[
    FixtureTool {
        key: "plumbing",
        kinds: &[Category::Plumbing, Category::Drain],
        letters: "PX",
        label: msg!("Plumbing Fixture"),
        title: msg!("Place Plumbing Fixture"),
        tip: msg!("Plumbing Fixture (PX): a basin, a WC, a sink, a shower or a drain, where you click; Space turns it"),
        icon: Icon::Fixture,
    },
    FixtureTool {
        key: "fitting",
        kinds: &[Category::PipeFitting],
        letters: "PF",
        label: msg!("Pipe Fitting"),
        title: msg!("Place Pipe Fitting"),
        tip: msg!("Pipe Fitting (PF): an elbow or a tee, where you click on a pipe's height; Space turns it"),
        icon: Icon::Fitting,
    },
    FixtureTool {
        key: "accessory",
        kinds: &[Category::PipeAccessory],
        letters: "PA",
        label: msg!("Pipe Accessory"),
        title: msg!("Place Pipe Accessory"),
        tip: msg!("Pipe Accessory (PA): a valve, where you click on a pipe's height; Space turns it"),
        icon: Icon::Accessory,
    },
    FixtureTool {
        key: "equipment",
        kinds: &[Category::Equipment],
        letters: "EE",
        label: "Electrical Equipment",
        title: msg!("Place Electrical Equipment"),
        tip: msg!("Electrical Equipment (EE): a distribution board, where you click on the wall; Space turns it"),
        icon: Icon::Equipment,
    },
    FixtureTool {
        key: "device",
        kinds: &[Category::Outlet, Category::Switch],
        letters: "DV",
        label: msg!("Device"),
        title: msg!("Place Device"),
        tip: msg!("Device (DV): an outlet or a switch, where you click on the wall; Space turns it"),
        icon: Icon::Device,
    },
    FixtureTool {
        key: "lighting",
        kinds: &[Category::Lighting],
        letters: "LF",
        label: msg!("Lighting Fixture"),
        title: msg!("Place Lighting Fixture"),
        tip: msg!("Lighting Fixture (LF): a luminaire, where you click, hung from the slab; Space turns it"),
        icon: Icon::Luminaire,
    },
    FixtureTool {
        key: "airterminal",
        kinds: &[Category::AirTerminal],
        letters: "AT",
        label: "Air Terminal",
        title: msg!("Place Air Terminal"),
        tip: msg!("Air Terminal (AT): a captor, where you click on the roof; Space turns it"),
        icon: Icon::AirTerminal,
    },
    FixtureTool {
        key: "earthing",
        kinds: &[Category::Earthing],
        letters: "EG",
        label: "Earthing Electrode",
        title: msg!("Place Earthing Electrode"),
        tip: msg!("Earthing Electrode (EG): a rod driven down from the level, where you click; Space turns it"),
        icon: Icon::Electrode,
    },
    FixtureTool {
        key: "inspection",
        kinds: &[Category::InspectionBox],
        letters: "IB",
        label: "Inspection Box",
        title: msg!("Place Inspection Box"),
        tip: msg!("Inspection Box (IB): over an electrode, flush with the level, where you click; Space turns it"),
        icon: Icon::InspectionBox,
    },
    FixtureTool {
        key: "dataoutlet",
        kinds: &[Category::DataOutlet],
        letters: "DT",
        label: "Data Outlet",
        title: msg!("Place Data Outlet"),
        tip: msg!("Data Outlet (DT): a data point, where you click on the wall; Space turns it"),
        icon: Icon::DataOutlet,
    },
    FixtureTool {
        key: "voiceoutlet",
        kinds: &[Category::VoiceOutlet],
        letters: "VO",
        label: "Voice Outlet",
        title: msg!("Place Voice Outlet"),
        tip: msg!("Voice Outlet (VO): a telephone point, where you click on the wall; Space turns it"),
        icon: Icon::VoiceOutlet,
    },
    FixtureTool {
        key: "rack",
        kinds: &[Category::Rack],
        letters: "RK",
        label: "Rack",
        title: msg!("Place Rack"),
        tip: msg!("Rack (RK): a floor rack, where you click; Space turns it"),
        icon: Icon::Rack,
    },
    FixtureTool {
        key: "patchpanel",
        kinds: &[Category::PatchPanel],
        letters: "PN",
        label: "Patch Panel",
        title: msg!("Place Patch Panel"),
        tip: msg!("Patch Panel (PN): a panel on the wall, where you click; Space turns it"),
        icon: Icon::PatchPanel,
    },
    FixtureTool {
        key: "alarmpanel",
        kinds: &[Category::AlarmPanel],
        letters: "FA",
        label: msg!("Fire Alarm Panel"),
        title: msg!("Place Fire Alarm Panel"),
        tip: msg!("Fire Alarm Panel (FA): a panel on the wall, where you click; Space turns it"),
        icon: Icon::Equipment,
    },
    FixtureTool {
        key: "smokedetector",
        kinds: &[Category::SmokeDetector],
        letters: "DS",
        label: msg!("Smoke Detector"),
        title: msg!("Place Smoke Detector"),
        tip: msg!("Smoke Detector (DS): a point detector on the ceiling, where you click; Space turns it"),
        icon: Icon::Device,
    },
    FixtureTool {
        key: "heatdetector",
        kinds: &[Category::HeatDetector],
        letters: "HD",
        label: msg!("Heat Detector"),
        title: msg!("Place Heat Detector"),
        tip: msg!("Heat Detector (HD): a point detector on the ceiling, where you click; Space turns it"),
        icon: Icon::Device,
    },
    FixtureTool {
        key: "callpoint",
        kinds: &[Category::CallPoint],
        letters: "CP",
        label: msg!("Manual Call Point"),
        title: msg!("Place Manual Call Point"),
        tip: msg!("Manual Call Point (CP): a call point on the wall, where you click; Space turns it"),
        icon: Icon::Device,
    },
    FixtureTool {
        key: "sounder",
        kinds: &[Category::Sounder],
        letters: "SR",
        label: msg!("Sounder"),
        title: msg!("Place Sounder"),
        tip: msg!("Sounder (SR): a bell or a siren on the wall, where you click; Space turns it"),
        icon: Icon::Device,
    },
    FixtureTool {
        key: "exitsign",
        kinds: &[Category::ExitSign],
        letters: "ES",
        label: msg!("Exit Sign"),
        title: msg!("Place Exit Sign"),
        tip: msg!("Exit Sign (ES): a light sign over a door, where you click on the wall; Space turns it"),
        icon: Icon::Luminaire,
    },
    FixtureTool {
        key: "speaker",
        kinds: &[Category::Speaker],
        letters: "AU",
        label: msg!("Ceiling Speaker"),
        title: msg!("Place Ceiling Speaker"),
        tip: msg!("Ceiling Speaker (AU): a speaker on the ceiling, where you click; Space turns it"),
        icon: Icon::Luminaire,
    },
    FixtureTool {
        key: "motiondetector",
        kinds: &[Category::MotionDetector],
        letters: "MO",
        label: msg!("Motion Detector"),
        title: msg!("Place Motion Detector"),
        tip: msg!("Motion Detector (MO): a sensor on the wall, where you click; Space turns it"),
        icon: Icon::Device,
    },
    FixtureTool {
        key: "camera",
        kinds: &[Category::Camera],
        letters: "VC",
        label: msg!("Camera"),
        title: msg!("Place Camera"),
        tip: msg!("Camera (VC): a CCTV camera on the wall, where you click; Space turns it"),
        icon: Icon::Device,
    },
];

pub(crate) fn fixture(key: &str) -> Option<&'static FixtureTool> {
    FIXTURES.iter().find(|f| f.key == key)
}

/// The fixture tool a two-letter shortcut picks.
pub(crate) fn fixture_with(letters: &str) -> Option<&'static FixtureTool> {
    FIXTURES.iter().find(|f| f.letters == letters)
}

/// The kinds of family the fixture tool `key` places.
pub(crate) fn kinds(key: &str) -> &'static [Category] {
    fixture(key).map_or(&[], |f| f.kinds)
}

impl BimApp {
    /// A panel's buttons: icon, label, tooltip and the tool each picks.
    fn buttons(&mut self, ui: &mut Ui, tools: &[(Icon, &str, &str, Tool)]) {
        for (icon, label, tip, tool) in tools {
            if big_button(ui, *icon, label, same_tool(&self.tool, tool), tip).clicked() {
                self.set_tool(tool.clone());
            }
        }
    }

    /// The button of the fixture tool `key`.
    fn fixture_button(key: &'static str) -> (Icon, &'static str, &'static str, Tool) {
        let f = fixture(key);
        (f.map_or(Icon::Component, |f| f.icon), f.map_or("", |f| f.label), f.map_or("", |f| f.tip), Tool::Fixture(key))
    }

    /// The Electrical tab: Revit's Electrical panel, a board, a device and a luminaire, and the conduit, tray and wire between.
    pub(crate) fn electrical_tab(&mut self, ui: &mut Ui) {
        self.tools(ui, "Select", &[(Icon::Modify, "Modify", "Modify (MD): select elements", Tool::Modify)]);
        panel(ui, "Electrical", |ui| {
            self.buttons(ui, &[Self::fixture_button("equipment"), Self::fixture_button("device"), Self::fixture_button("lighting")]);
        });
        panel(ui, msg!("Cable Carrier & Wire"), |ui| {
            let carrier = (
                Icon::CableTray,
                "Cable Carrier",
                msg!("Cable Carrier (CC): click its start and its end; a chain of clicks makes a carrier for each step"),
                Tool::Run("cablecarrier", None),
            );
            let wire = (
                Icon::Wire,
                "Wire",
                msg!("Wire (EW): click its start and its end; a chain of clicks makes a wire for each step"),
                Tool::Run("wire", None),
            );
            self.buttons(ui, &[carrier, wire]);
        });
    }

    /// The Data tab (lógica, dados e voz, NBR 14565): its outlets, its rack and panels, and the trays and cabling between.
    pub(crate) fn data_tab(&mut self, ui: &mut Ui) {
        self.tools(ui, "Select", &[(Icon::Modify, "Modify", "Modify (MD): select elements", Tool::Modify)]);
        panel(ui, msg!("Outlets & Equipment"), |ui| {
            self.buttons(
                ui,
                &[
                    Self::fixture_button("dataoutlet"),
                    Self::fixture_button("voiceoutlet"),
                    Self::fixture_button("rack"),
                    Self::fixture_button("patchpanel"),
                ],
            );
        });
        panel(ui, "Cabling", |ui| {
            let carrier = (
                Icon::CableTray,
                "Cable Carrier",
                "Cable Carrier (CC): click its start and its end; a chain of clicks makes a carrier for each step",
                Tool::Run("cablecarrier", None),
            );
            let cabling = (
                Icon::Cabling,
                "Cabling",
                msg!("Cabling (CB): click its start and its end; a chain of clicks makes a cable for each step"),
                Tool::Run("cable", None),
            );
            self.buttons(ui, &[carrier, cabling]);
        });
    }

    /// The Low Voltage tab (fire alarm, voice evacuation, security): its panel, its detectors and alarm appliances, its
    /// security devices, and the circuit that runs between them.
    pub(crate) fn low_voltage_tab(&mut self, ui: &mut Ui) {
        self.tools(ui, "Select", &[(Icon::Modify, "Modify", "Modify (MD): select elements", Tool::Modify)]);
        panel(ui, msg!("Fire Alarm"), |ui| {
            self.buttons(
                ui,
                &[
                    Self::fixture_button("alarmpanel"),
                    Self::fixture_button("smokedetector"),
                    Self::fixture_button("heatdetector"),
                    Self::fixture_button("callpoint"),
                ],
            );
        });
        panel(ui, msg!("Alarm & Voice"), |ui| {
            self.buttons(ui, &[Self::fixture_button("sounder"), Self::fixture_button("exitsign"), Self::fixture_button("speaker")]);
        });
        panel(ui, msg!("Security"), |ui| {
            self.buttons(ui, &[Self::fixture_button("motiondetector"), Self::fixture_button("camera")]);
        });
        panel(ui, msg!("Wires"), |ui| {
            self.buttons(
                ui,
                &[(
                    Icon::Wire,
                    "Wire",
                    msg!("Wire (EW): click its start and its end; a chain of clicks makes a wire for each step"),
                    Tool::Run("wire", None),
                )],
            );
        });
    }

    /// The Lightning Protection tab (SPDA, NBR 5419): what captures, what carries the current down, and what earths it. Revit
    /// has none, so these are BIMCraft's own, on the same runs and fixtures.
    pub(crate) fn lightning_tab(&mut self, ui: &mut Ui) {
        self.tools(ui, "Select", &[(Icon::Modify, "Modify", "Modify (MD): select elements", Tool::Modify)]);
        panel(ui, msg!("Capture & Descent"), |ui| {
            let down = (
                Icon::DownConductor,
                "Down Conductor",
                msg!("Down Conductor (DC): click its start and its end; the same point twice and two heights stand it up a wall"),
                Tool::Run("downconductor", None),
            );
            self.buttons(ui, &[Self::fixture_button("airterminal"), down]);
        });
        panel(ui, msg!("Earthing"), |ui| {
            let ground = (
                Icon::GroundConductor,
                "Earthing Conductor",
                msg!("Earthing Conductor (GC): click its start and its end; a chain of clicks makes the ring or the mesh, under the level"),
                Tool::Run("groundconductor", None),
            );
            self.buttons(ui, &[Self::fixture_button("earthing"), Self::fixture_button("inspection"), ground]);
        });
    }

    /// The Plumbing tab: Revit's Plumbing & Piping panel, a pipe, the fixtures that go on it and what joins and stops it.
    pub(crate) fn plumbing_tab(&mut self, ui: &mut Ui) {
        self.tools(ui, "Select", &[(Icon::Modify, "Modify", "Modify (MD): select elements", Tool::Modify)]);
        panel(ui, msg!("Plumbing & Piping"), |ui| {
            let pipe = (
                Icon::Pipe,
                "Pipe",
                msg!("Pipe (PI): click its start and its end; a chain of clicks makes a pipe for each step"),
                Tool::Run("pipe", None),
            );
            self.buttons(ui, &[pipe, Self::fixture_button("plumbing"), Self::fixture_button("fitting"), Self::fixture_button("accessory")]);
        });
    }
}
