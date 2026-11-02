# Low-voltage systems: from a model to a set of drawings

The aim: BIMCraft as a Renga-like modeller for low-voltage design (fire alarm, voice evacuation, security, CCTV, access
control, structured cabling) that ends in the full set of documents a Russian project hands over (GOST R 21.101, GOST 21.110,
SP 484, SP 3.13130). Each step is a branch of its own, tested, documented in `docs/areas/`.

| # | Step | What it gives | State |
|---|------|---------------|-------|
| 1 | Wires (`Element::Wire`) | physical cable with optional source and ordered devices, reserve and a cable schedule | done; one element for physical and logical data |
| 2 | Russian project language | families, schedules, title blocks and names in Russian | done (the project; the window's own words are still English) |
| 3 | Low-voltage families | a fire alarm panel, smoke, heat and manual detectors, sounders, a light sign, a speaker, a motion detector, a camera; their IFC classes | done (the Low Voltage tab) |
| 4 | Wires on plans | physical axes in plan; device addresses from the system label and ordered device list | physical run rendering; logical wall-following length calculation via `along_walls` |
| 5 | Wire and Cable Carrier tools | place cables and round or rectangular carriers with one shared run tool | EW and CC, on discipline tabs; logical references via commands |
| 6 | Automation | lay detectors by room area and ceiling height (SP 484), panel limits, carrier-aware routing | `detectors` and addresses implemented; panel limits and carrier-driven routing pending |
| 7 | IFC | physical cables and carriers, then logical ports and connections | `IfcCableSegment` and `IfcCableCarrierSegment`; logical ports pending |
| 8 | The documents | the structural diagram, the cable journal, the specification of equipment (GOST 21.110), a sheet set with GOST title blocks (form 3 and 6), PDF and DWG | |

## Standards packages and current scope

Standards and drawing conventions are intended to be replaceable, versioned modules: calculations and checks, symbols, specification templates, and document formatting. The geometric core is shared. The next contributions add carrier geometry and a bounded Russian box maximum-fill profile; neither is part of this ODS boundary. American, European, and other national packages are not implemented. A general loadable package interface remains future work.

Automatic creation of carriers from a cable, inherited elevations, and device-to-route drops/rises are planned, not implemented here. Universal device ports are also future work.
