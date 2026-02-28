use crate::ui::NetTemplateEntry;
use crate::ui::cmd_history::CmdHistory;
use crate::ui::core::requests::ShellConfirm;
use crate::ui::core::types::{DeployMarker, InspectLine, InspectMode, LogsMode, TemplateEntry};
use crate::ui::theme;
use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use time::OffsetDateTime;

#[derive(Debug, Default, Clone)]
pub(in crate::ui) struct ShellCmdlineState {
    pub(in crate::ui) mode: bool,
    pub(in crate::ui) input: String,
    pub(in crate::ui) cursor: usize,
    pub(in crate::ui) confirm: Option<ShellConfirm>,
    pub(in crate::ui) history: CmdHistory,
}

#[derive(Debug, Clone)]
pub(in crate::ui) struct ShellMessagesState {
    pub(in crate::ui) scroll: usize, // cursor (absolute); usize::MAX = last
    pub(in crate::ui) hscroll: usize, // horizontal scroll
    pub(in crate::ui) return_view: ShellView,
}

#[derive(Debug, Clone)]
pub(in crate::ui) struct ShellHelpState {
    pub(in crate::ui) scroll: usize,
    pub(in crate::ui) return_view: ShellView,
}

#[derive(Debug, Clone)]
pub(in crate::ui) struct ThemeSelectorState {
    pub(in crate::ui) names: Vec<String>,
    pub(in crate::ui) selected: usize,
    pub(in crate::ui) scroll: usize,
    pub(in crate::ui) page_size: usize,
    pub(in crate::ui) center_on_open: bool,
    pub(in crate::ui) return_view: ShellView,
    pub(in crate::ui) base_theme_name: String,
    pub(in crate::ui) preview_theme: theme::ThemeSpec,
    pub(in crate::ui) error: Option<String>,
    pub(in crate::ui) search_mode: bool,
    pub(in crate::ui) search_input: String,
    pub(in crate::ui) search_cursor: usize,
}

#[derive(Debug, Clone)]
pub(in crate::ui) struct InspectState {
    pub(in crate::ui) loading: bool,
    pub(in crate::ui) error: Option<String>,
    pub(in crate::ui) value: Option<Value>,
    pub(in crate::ui) target: Option<crate::ui::InspectTarget>,
    pub(in crate::ui) for_id: Option<String>,
    pub(in crate::ui) lines: Vec<InspectLine>,
    pub(in crate::ui) selected: usize,
    pub(in crate::ui) scroll_top: usize,
    pub(in crate::ui) scroll: usize,
    pub(in crate::ui) query: String,
    pub(in crate::ui) expanded: HashSet<String>,
    pub(in crate::ui) match_paths: Vec<String>,
    pub(in crate::ui) path_rank: HashMap<String, usize>,
    pub(in crate::ui) mode: InspectMode,
    pub(in crate::ui) input: String,
    pub(in crate::ui) input_cursor: usize,
    pub(in crate::ui) cmd_history: CmdHistory,
}

#[derive(Debug, Clone)]
pub(in crate::ui) struct LogsState {
    pub(in crate::ui) loading: bool,
    pub(in crate::ui) error: Option<String>,
    pub(in crate::ui) text: Option<String>,
    pub(in crate::ui) for_id: Option<String>,
    pub(in crate::ui) tail: usize,
    pub(in crate::ui) cursor: usize,
    pub(in crate::ui) scroll_top: usize,
    pub(in crate::ui) select_anchor: Option<usize>,
    pub(in crate::ui) hscroll: usize,
    pub(in crate::ui) max_width: usize,
    pub(in crate::ui) mode: LogsMode,
    pub(in crate::ui) input: String,
    pub(in crate::ui) query: String,
    pub(in crate::ui) command: String,
    pub(in crate::ui) input_cursor: usize,
    pub(in crate::ui) command_cursor: usize,
    pub(in crate::ui) cmd_history: CmdHistory,
    pub(in crate::ui) use_regex: bool,
    pub(in crate::ui) regex: Option<Regex>,
    pub(in crate::ui) regex_error: Option<String>,
    pub(in crate::ui) match_lines: Vec<usize>,
    pub(in crate::ui) show_line_numbers: bool,
}

#[derive(Debug, Clone)]
pub(in crate::ui) struct TemplatesState {
    pub(in crate::ui) dir: PathBuf,
    pub(in crate::ui) kind: TemplatesKind,

    pub(in crate::ui) templates: Vec<TemplateEntry>,
    pub(in crate::ui) templates_selected: usize,
    pub(in crate::ui) templates_error: Option<String>,
    pub(in crate::ui) templates_details_scroll: usize,
    pub(in crate::ui) templates_refresh_after_edit: Option<String>,
    pub(in crate::ui) template_deploy_inflight: HashMap<String, DeployMarker>,
    pub(in crate::ui) git_head: Option<String>,
    pub(in crate::ui) git_remote_templates: HashMap<String, GitRemoteStatus>,
    pub(in crate::ui) dirty_templates: HashSet<String>,
    pub(in crate::ui) untracked_templates: HashSet<String>,

    pub(in crate::ui) net_templates: Vec<NetTemplateEntry>,
    pub(in crate::ui) net_templates_selected: usize,
    pub(in crate::ui) net_templates_error: Option<String>,
    pub(in crate::ui) net_templates_details_scroll: usize,
    pub(in crate::ui) net_templates_refresh_after_edit: Option<String>,
    pub(in crate::ui) net_template_deploy_inflight: HashMap<String, DeployMarker>,
    pub(in crate::ui) dirty_net_templates: HashSet<String>,
    pub(in crate::ui) untracked_net_templates: HashSet<String>,
    pub(in crate::ui) git_remote_net_templates: HashMap<String, GitRemoteStatus>,
    pub(in crate::ui) ai_edit_snapshot: Option<TemplateEditSnapshot>,
}

#[derive(Clone, Debug)]
pub(in crate::ui) struct TemplateEditSnapshot {
    pub(in crate::ui) kind: TemplatesKind,
    pub(in crate::ui) name: String,
    pub(in crate::ui) path: PathBuf,
    pub(in crate::ui) hash: Option<u64>,
}

#[allow(private_interfaces)]
pub(in crate::ui) fn shell_begin_confirm(
    app: &mut crate::ui::App,
    label: impl Into<String>,
    cmdline: impl Into<String>,
) {
    app.shell_cmdline.mode = true;
    app.shell_cmdline.input.clear();
    app.shell_cmdline.cursor = 0;
    app.shell_cmdline.confirm = Some(ShellConfirm {
        label: label.into(),
        cmdline: cmdline.into(),
    });
}

pub(in crate::ui) fn input_window_with_cursor(
    text: &str,
    cursor: usize,
    width: usize,
) -> (String, String, String) {
    let width = width.max(1);
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let cursor = cursor.min(len);

    if len <= width {
        let before: String = chars.iter().take(cursor).collect();
        let at = if cursor < len {
            chars[cursor].to_string()
        } else {
            " ".to_string()
        };
        let after: String = chars.iter().skip(cursor.saturating_add(1)).collect();
        return (before, at, after);
    }

    let mut start = 0usize;
    if cursor >= width {
        start = cursor - width + 1;
    }
    if start + width > len {
        start = len - width;
    }
    let end = (start + width).min(len);
    let rel = cursor.saturating_sub(start).min(end - start);
    let before: String = chars[start..start + rel].iter().collect();
    let at = if start + rel < end {
        chars[start + rel].to_string()
    } else {
        " ".to_string()
    };
    let after_start = (start + rel + 1).min(end);
    let after: String = chars[after_start..end].iter().collect();
    (before, at, after)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum TemplatesKind {
    Stacks,
    Networks,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum GitRemoteStatus {
    Unknown,
    UpToDate,
    Ahead,
    Behind,
    Diverged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum ListMode {
    Flat,
    Tree,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum ActiveView {
    Containers,
    Stacks,
    Images,
    Volumes,
    Networks,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::ui) enum ShellView {
    Dashboard,
    Stacks,
    Containers,
    Images,
    Volumes,
    Networks,
    Templates,
    Registries,
    Inspect,
    Logs,
    Help,
    Messages,
    ThemeSelector,
}

impl ShellView {
    pub(in crate::ui) fn slug(self) -> &'static str {
        match self {
            ShellView::Dashboard => "dashboard",
            ShellView::Stacks => "stacks",
            ShellView::Containers => "containers",
            ShellView::Images => "images",
            ShellView::Volumes => "volumes",
            ShellView::Networks => "networks",
            ShellView::Templates => "templates",
            ShellView::Registries => "registries",
            ShellView::Inspect => "inspect",
            ShellView::Logs => "logs",
            ShellView::Help => "help",
            ShellView::Messages => "messages",
            ShellView::ThemeSelector => "themes",
        }
    }

    pub(in crate::ui) fn title(self) -> &'static str {
        match self {
            ShellView::Dashboard => "Dashboard",
            ShellView::Stacks => "Stacks",
            ShellView::Containers => "Containers",
            ShellView::Images => "Images",
            ShellView::Volumes => "Volumes",
            ShellView::Networks => "Networks",
            ShellView::Templates => "Templates",
            ShellView::Registries => "Registries",
            ShellView::Inspect => "Inspect",
            ShellView::Logs => "Logs",
            ShellView::Help => "Help",
            ShellView::Messages => "Messages",
            ShellView::ThemeSelector => "Themes",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum ShellFocus {
    Sidebar,
    List,
    Details,
    Dock,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum ShellSplitMode {
    Horizontal,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum ShellSidebarItem {
    Separator,
    Gap,
    Server(usize),
    Module(ShellView),
    Action(ShellAction),
}

#[derive(Clone, Debug)]
pub(in crate::ui) enum ShellInteractive {
    RunCommand { cmd: String },
    RunLocalCommand { cmd: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum MsgLevel {
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug)]
pub(in crate::ui) struct SessionMsg {
    pub(in crate::ui) at: OffsetDateTime,
    pub(in crate::ui) level: MsgLevel,
    pub(in crate::ui) text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum ShellAction {
    Inspect,
    Logs,
    Start,
    Stop,
    Restart,
    Delete,
    StackUpdate,
    StackUpdateAll,
    Console,
    ImageUntag,
    ImageForceRemove,
    VolumeRemove,
    NetworkRemove,
    RegistryTest,
    TemplateAi,
    TemplateEdit,
    TemplateNew,
    TemplateDelete,
    TemplateDeploy,
    TemplateRedeploy,
}

impl ShellAction {
    pub(in crate::ui) fn label(self) -> &'static str {
        match self {
            ShellAction::Inspect => "Inspect",
            ShellAction::Logs => "Logs",
            ShellAction::Start => "Start",
            ShellAction::Stop => "Stop",
            ShellAction::Restart => "Restart",
            ShellAction::Delete => "Delete",
            ShellAction::StackUpdate => "Update",
            ShellAction::StackUpdateAll => "Update all",
            ShellAction::Console => "Console",
            ShellAction::ImageUntag => "Untag",
            ShellAction::ImageForceRemove => "Remove",
            ShellAction::VolumeRemove => "Remove",
            ShellAction::NetworkRemove => "Remove",
            ShellAction::RegistryTest => "Test",
            ShellAction::TemplateAi => "AI",
            ShellAction::TemplateEdit => "Edit",
            ShellAction::TemplateNew => "New",
            ShellAction::TemplateDelete => "Delete",
            ShellAction::TemplateDeploy => "Deploy",
            ShellAction::TemplateRedeploy => "Redeploy",
        }
    }

    pub(in crate::ui) fn ctrl_hint(self) -> &'static str {
        match self {
            ShellAction::Inspect => "^i",
            ShellAction::Logs => "^l",
            ShellAction::Start => "^s",
            ShellAction::Stop => "^o",
            ShellAction::Restart => "^r",
            ShellAction::Delete => "^d",
            ShellAction::StackUpdate => "^u",
            ShellAction::StackUpdateAll => "^U",
            ShellAction::Console => "^c",
            ShellAction::ImageUntag => "^u",
            ShellAction::ImageForceRemove => "^d",
            ShellAction::VolumeRemove => "^d",
            ShellAction::NetworkRemove => "^d",
            ShellAction::RegistryTest => "^y",
            ShellAction::TemplateAi => "^a",
            ShellAction::TemplateEdit => "^e",
            ShellAction::TemplateNew => "^n",
            ShellAction::TemplateDelete => "^d",
            ShellAction::TemplateDeploy => "^y",
            ShellAction::TemplateRedeploy => "^Y",
        }
    }
}
