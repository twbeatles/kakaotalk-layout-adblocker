use std::path::PathBuf;

pub const VERSION: &str = "11.1.4";
pub const APPDATA_DIRNAME: &str = "KakaoTalkAdBlockerLayout";
pub const SETTINGS_FILE: &str = "layout_settings_v11.json";
pub const RULES_FILE: &str = "layout_rules_v11.json";
pub const LOG_FILE: &str = "layout_adblock.log";
pub const UPDATE_PUBLIC_KEY_B64: &str = "Cix9d2r5UZxpDL4Bp9CWNrjMDRTQHF5Y1snTMYnMQ2U=";

#[derive(Debug, Clone)]
pub struct RuntimePaths {
    pub appdata_dir: PathBuf,
    pub settings_file: PathBuf,
    pub rules_file: PathBuf,
    pub log_file: PathBuf,
}

pub fn runtime_paths() -> RuntimePaths {
    let appdata = std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(".").join("AppData").join("Roaming"));
    let appdata_dir = appdata.join(APPDATA_DIRNAME);
    RuntimePaths {
        settings_file: appdata_dir.join(SETTINGS_FILE),
        rules_file: appdata_dir.join(RULES_FILE),
        log_file: appdata_dir.join(LOG_FILE),
        appdata_dir,
    }
}
