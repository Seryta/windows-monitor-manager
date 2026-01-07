pub mod display;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MonitorInfo {
    pub id: u32, // Logical index for simple display
    pub name: String,
    pub device_string: String,
    pub is_active: bool,
    pub is_attached: bool,
    pub is_primary: bool,
    // CCD Identification
    pub adapter_id_low: u32,
    pub adapter_id_high: u32,
    pub target_id: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TopologyMode {
    Internal,
    External,
    Extend,
    Clone,
    // String format: "AdapterLow:AdapterHigh:TargetId"
    Single(String), 
}
