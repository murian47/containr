//! Shared UI/core data types.
//!
//! This module holds lightweight types exchanged between state, background tasks, rendering, and
//! input handling. Keep it free of business logic except for tiny classification helpers.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::config;
use crate::docker::ContainerAction;

#[derive(Clone, Debug)]
pub(in crate::ui) enum ViewEntry {
    StackHeader {
        name: String,
        total: usize,
        running: usize,
        expanded: bool,
    },
    UngroupedHeader {
        total: usize,
        running: usize,
    },
    Container {
        id: String,
        indent: usize,
    },
}

#[derive(Clone, Debug)]
pub(in crate::ui) struct StackEntry {
    pub(in crate::ui) name: String,
    pub(in crate::ui) total: usize,
    pub(in crate::ui) running: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum StackDetailsFocus {
    Containers,
    Networks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::ui) enum InspectMode {
    Normal,
    Search,
    Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::ui) enum LogsMode {
    Normal,
    Search,
    Command,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::ui) enum InspectKind {
    Container,
    Image,
    Volume,
    Network,
}

#[derive(Debug, Clone)]
pub(in crate::ui) struct InspectTarget {
    pub(in crate::ui) kind: InspectKind,
    pub(in crate::ui) key: String,
    pub(in crate::ui) arg: String,
    pub(in crate::ui) label: String,
}

#[derive(Debug, Clone)]
pub(in crate::ui) struct InspectLine {
    pub(in crate::ui) path: String,
    pub(in crate::ui) depth: usize,
    pub(in crate::ui) label: String,
    pub(in crate::ui) summary: String,
    pub(in crate::ui) expandable: bool,
    pub(in crate::ui) expanded: bool,
    pub(in crate::ui) matches: bool,
}

#[derive(Clone, Debug)]
pub(in crate::ui) struct TemplateEntry {
    pub(in crate::ui) name: String,
    pub(in crate::ui) dir: PathBuf,
    pub(in crate::ui) compose_path: PathBuf,
    pub(in crate::ui) has_compose: bool,
    pub(in crate::ui) desc: String,
    pub(in crate::ui) template_id: Option<String>,
}

#[derive(Clone, Debug)]
pub(in crate::ui) struct NetTemplateEntry {
    pub(in crate::ui) name: String,
    pub(in crate::ui) dir: PathBuf,
    pub(in crate::ui) cfg_path: PathBuf,
    pub(in crate::ui) has_cfg: bool,
    pub(in crate::ui) desc: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(in crate::ui) struct NetworkTemplateIpv4 {
    pub(in crate::ui) subnet: Option<String>,
    pub(in crate::ui) gateway: Option<String>,
    #[serde(rename = "ip_range")]
    pub(in crate::ui) ip_range: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(in crate::ui) struct NetworkTemplateSpec {
    pub(in crate::ui) name: String,
    #[allow(dead_code)]
    #[serde(default)]
    pub(in crate::ui) description: Option<String>,
    #[serde(default)]
    pub(in crate::ui) driver: Option<String>,
    #[serde(default)]
    pub(in crate::ui) parent: Option<String>,
    #[serde(default, rename = "ipvlan_mode")]
    pub(in crate::ui) ipvlan_mode: Option<String>,
    #[serde(default)]
    pub(in crate::ui) internal: Option<bool>,
    #[serde(default)]
    pub(in crate::ui) attachable: Option<bool>,
    #[serde(default)]
    pub(in crate::ui) ipv4: Option<NetworkTemplateIpv4>,
    #[serde(default)]
    pub(in crate::ui) options: Option<HashMap<String, String>>,
    #[serde(default)]
    pub(in crate::ui) labels: Option<HashMap<String, String>>,
}

pub(in crate::ui) const IMAGE_UPDATE_TTL_SECS: i64 = 24 * 60 * 60;
pub(in crate::ui) const RATE_LIMIT_WINDOW_SECS: i64 = 6 * 60 * 60;
pub(in crate::ui) const RATE_LIMIT_MAX: usize = 100;
pub(in crate::ui) const RATE_LIMIT_WARN: usize = 80;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(in crate::ui) enum ImageUpdateKind {
    UpToDate,
    UpdateAvailable,
    Error,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(in crate::ui) struct ImageUpdateEntry {
    pub(in crate::ui) checked_at: i64,
    pub(in crate::ui) status: ImageUpdateKind,
    pub(in crate::ui) local_digest: Option<String>,
    pub(in crate::ui) remote_digest: Option<String>,
    #[serde(default)]
    pub(in crate::ui) note: Option<String>,
    pub(in crate::ui) error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(in crate::ui) struct TemplateDeployEntry {
    pub(in crate::ui) server_name: String,
    pub(in crate::ui) timestamp: i64,
    #[serde(default)]
    pub(in crate::ui) commit: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(in crate::ui) struct RegistryTestEntry {
    pub(in crate::ui) checked_at: i64,
    pub(in crate::ui) ok: bool,
    pub(in crate::ui) message: String,
}

#[derive(Clone, Debug)]
pub(in crate::ui) struct RegistryAuthResolved {
    pub(in crate::ui) auth: config::RegistryAuth,
    pub(in crate::ui) username: Option<String>,
    pub(in crate::ui) secret: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub(in crate::ui) struct RateLimitEntry {
    pub(in crate::ui) hits: Vec<i64>,
    pub(in crate::ui) limited_until: Option<i64>,
}

#[derive(Default, Serialize, Deserialize)]
pub(in crate::ui) struct LocalState {
    pub(in crate::ui) version: u32,
    #[serde(default)]
    pub(in crate::ui) image_updates: HashMap<String, ImageUpdateEntry>,
    #[serde(default)]
    pub(in crate::ui) rate_limits: HashMap<String, RateLimitEntry>,
    #[serde(default)]
    pub(in crate::ui) template_deploys: HashMap<String, Vec<TemplateDeployEntry>>,
    #[serde(default)]
    pub(in crate::ui) net_template_deploys: HashMap<String, Vec<TemplateDeployEntry>>,
    #[serde(default)]
    pub(in crate::ui) registry_tests: HashMap<String, RegistryTestEntry>,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::ui) struct DeployMarker {
    pub(in crate::ui) started: Instant,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::ui) struct ActionMarker {
    pub(in crate::ui) action: ContainerAction,
    pub(in crate::ui) until: Instant,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::ui) struct SimpleMarker {
    pub(in crate::ui) until: Instant,
}

#[derive(Clone, Debug)]
pub(in crate::ui) enum ActionErrorKind {
    InUse,
    Other,
}

#[derive(Clone, Debug)]
pub(in crate::ui) struct LastActionError {
    pub(in crate::ui) at: OffsetDateTime,
    pub(in crate::ui) action: String,
    pub(in crate::ui) kind: ActionErrorKind,
    pub(in crate::ui) message: String,
}

#[derive(Clone, Debug)]
pub(in crate::ui) struct StackUpdateService {
    pub(in crate::ui) name: String,
    pub(in crate::ui) container_id: String,
    pub(in crate::ui) image: String,
}

pub(in crate::ui) fn classify_action_error(msg: &str) -> ActionErrorKind {
    let s = msg.to_ascii_lowercase();
    if s.contains("in use")
        || s.contains("being used")
        || s.contains("has active endpoints")
        || s.contains("active endpoints")
        || s.contains("is being used")
    {
        ActionErrorKind::InUse
    } else {
        ActionErrorKind::Other
    }
}

#[derive(Debug, Clone, Default)]
pub(in crate::ui) struct UsageSnapshot {
    pub(in crate::ui) image_ref_count_by_id: HashMap<String, usize>,
    pub(in crate::ui) image_run_count_by_id: HashMap<String, usize>,
    pub(in crate::ui) image_containers_by_id: HashMap<String, Vec<String>>,
    pub(in crate::ui) volume_ref_count_by_name: HashMap<String, usize>,
    pub(in crate::ui) volume_run_count_by_name: HashMap<String, usize>,
    pub(in crate::ui) volume_containers_by_name: HashMap<String, Vec<String>>,
    pub(in crate::ui) network_ref_count_by_id: HashMap<String, usize>,
    pub(in crate::ui) network_containers_by_id: HashMap<String, Vec<String>>,
    pub(in crate::ui) ip_by_container_id: HashMap<String, String>,
}

#[derive(Clone, Debug)]
pub(in crate::ui) struct DashboardSnapshot {
    pub(in crate::ui) os: String,
    pub(in crate::ui) kernel: String,
    pub(in crate::ui) arch: String,
    pub(in crate::ui) uptime: String,
    pub(in crate::ui) engine: String,
    pub(in crate::ui) containers_running: u32,
    pub(in crate::ui) containers_total: u32,
    pub(in crate::ui) cpu_cores: u32,
    pub(in crate::ui) load1: f32,
    pub(in crate::ui) load5: f32,
    pub(in crate::ui) load15: f32,
    pub(in crate::ui) mem_used_bytes: u64,
    pub(in crate::ui) mem_total_bytes: u64,
    pub(in crate::ui) disk_used_bytes: u64,
    pub(in crate::ui) disk_total_bytes: u64,
    pub(in crate::ui) disks: Vec<DiskEntry>,
    pub(in crate::ui) nics: Vec<NicEntry>,
    pub(in crate::ui) collected_at: OffsetDateTime,
}

#[derive(Clone, Debug, Default)]
pub(in crate::ui) struct DashboardState {
    pub(in crate::ui) loading: bool,
    pub(in crate::ui) error: Option<String>,
    pub(in crate::ui) snap: Option<DashboardSnapshot>,
    pub(in crate::ui) last_disk_count: usize,
    pub(in crate::ui) suppress_image_frames: u8,
}

#[derive(Clone, Debug)]
pub(in crate::ui) struct DashboardHostState {
    pub(in crate::ui) name: String,
    pub(in crate::ui) loading: bool,
    pub(in crate::ui) error: Option<String>,
    pub(in crate::ui) snap: Option<DashboardSnapshot>,
    pub(in crate::ui) latency_ms: Option<u128>,
}

#[derive(Clone, Debug, Default)]
pub(in crate::ui) struct DashboardAllState {
    pub(in crate::ui) hosts: Vec<DashboardHostState>,
    pub(in crate::ui) scroll_top: usize,
    pub(in crate::ui) page_rows: usize,
}

pub(in crate::ui) struct DashboardImageState {
    pub(in crate::ui) enabled: bool,
    pub(in crate::ui) picker: Picker,
    pub(in crate::ui) protocol: Option<StatefulProtocol>,
    pub(in crate::ui) last_key: Option<String>,
}

#[derive(Clone, Debug)]
pub(in crate::ui) struct DiskEntry {
    pub(in crate::ui) source: String,
    pub(in crate::ui) fs_type: String,
    pub(in crate::ui) mount: String,
    pub(in crate::ui) used_bytes: u64,
    pub(in crate::ui) total_bytes: u64,
}

#[derive(Clone, Debug)]
pub(in crate::ui) struct NicEntry {
    pub(in crate::ui) name: String,
    pub(in crate::ui) addr: String,
}
