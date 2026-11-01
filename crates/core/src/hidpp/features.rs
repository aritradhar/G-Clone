//! HID++ 2.0 feature ids used by this app.

pub const ROOT: u16 = 0x0000;
pub const FEATURE_SET: u16 = 0x0001;
pub const DEVICE_FW_INFO: u16 = 0x0003;
pub const DEVICE_NAME: u16 = 0x0005;
pub const BATTERY_STATUS: u16 = 0x1000;
pub const UNIFIED_BATTERY: u16 = 0x1004;
pub const LED_CONTROL: u16 = 0x1300;
pub const ILLUMINATION: u16 = 0x1990;
pub const REPROG_CONTROLS_V4: u16 = 0x1B04;
pub const ADJUSTABLE_DPI: u16 = 0x2201;
pub const EXTENDED_DPI: u16 = 0x2202;
pub const REPORT_RATE: u16 = 0x8060;
pub const EXTENDED_REPORT_RATE: u16 = 0x8061;
pub const COLOR_LED_EFFECTS: u16 = 0x8070;
pub const RGB_EFFECTS: u16 = 0x8071;
pub const ONBOARD_PROFILES: u16 = 0x8100;
pub const MOUSE_BUTTON_SPY: u16 = 0x8110;
pub const FORCE_FEEDBACK: u16 = 0x8123;
/// Undocumented; per-axis sensitivity on the G923 (what G HUB's sensitivity sliders write).
pub const AXIS_SENSITIVITY: u16 = 0x80A3;

pub fn name(id: u16) -> &'static str {
    match id {
        0x0000 => "Root",
        0x0001 => "FeatureSet",
        0x0002 => "FeatureInfo",
        0x0003 => "DeviceFwVersion",
        0x0005 => "DeviceName",
        0x0007 => "DeviceFriendlyName",
        0x0020 => "ConfigChange",
        0x00C2 => "DfuControlSigned",
        0x00D0 => "DFU",
        0x1000 => "BatteryStatus",
        0x1001 => "BatteryVoltage",
        0x1004 => "UnifiedBattery",
        0x1300 => "LedControl",
        0x1802 => "DeviceReset",
        0x1990 => "Illumination",
        0x1B04 => "ReprogControlsV4",
        0x1D4B => "WirelessDeviceStatus",
        0x1F20 => "AdcMeasurement",
        0x2200 => "MousePointer",
        0x2201 => "AdjustableDpi",
        0x2202 => "ExtendedAdjustableDpi",
        0x2205 => "PointerMotionScaling",
        0x8010 => "GKey",
        0x8020 => "MKeys",
        0x8030 => "MR",
        0x8040 => "BrightnessControl",
        0x8060 => "AdjustableReportRate",
        0x8061 => "ExtendedAdjustableReportRate",
        0x8070 => "ColorLedEffects",
        0x8071 => "RgbEffects",
        0x8080 => "PerKeyLighting",
        0x8090 => "ModeStatus",
        0x80A3 => "AxisSensitivity (undocumented)",
        0x8100 => "OnboardProfiles",
        0x8110 => "MouseButtonSpy",
        0x8111 => "LatencyMonitoring",
        0x8120 => "GamingAttachments",
        0x8123 => "ForceFeedback",
        0x8127 => "AxisResponseCurve",
        0x8133 => "Combined Pedals",
        0x8134 => "BrakeForce",
        0x8135 => "PedalStatus",
        0x8136 => "ConfigurableAxes",
        0x8137 => "WheelCenterPosition",
        0x8138 => "FfbFilter",
        0x8139 => "TrueForce",
        _ => "",
    }
}
