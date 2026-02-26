use std::path::PathBuf;

use crate::docker::{ContainerAction, DockerCfg};
use crate::runner::Runner;

use super::{RegistryAuthResolved, StackUpdateService};

#[derive(Debug, Clone)]
pub(in crate::ui) enum ActionRequest {
    Container {
        action: ContainerAction,
        id: String,
    },
    RegistryTest {
        host: String,
        auth: RegistryAuthResolved,
        test_repo: Option<String>,
    },
    TemplateDeploy {
        name: String,
        runner: Runner,
        docker: DockerCfg,
        local_compose: PathBuf,
        pull: bool,
        force_recreate: bool,
        server_name: String,
        template_id: String,
        template_commit: Option<String>,
    },
    StackUpdate {
        stack_name: String,
        runner: Runner,
        docker: DockerCfg,
        compose_dirs: Vec<String>,
        pull: bool,
        dry: bool,
        force: bool,
        services: Vec<StackUpdateService>,
    },
    NetTemplateDeploy {
        name: String,
        runner: Runner,
        docker: DockerCfg,
        local_cfg: PathBuf,
        force: bool,
        server_name: String,
    },
    TemplateFromNetwork {
        name: String,
        source: String,
        network_id: String,
        templates_dir: PathBuf,
    },
    TemplateFromStack {
        name: String,
        stack_name: String,
        source: String,
        container_ids: Vec<String>,
        templates_dir: PathBuf,
    },
    TemplateFromContainer {
        name: String,
        source: String,
        container_id: String,
        templates_dir: PathBuf,
    },
    ImageUpdateCheck {
        image: String,
        debug: bool,
    },
    ImageUntag {
        marker_key: String,
        reference: String,
    },
    ImageForceRemove {
        marker_key: String,
        id: String,
    },
    ImagePush {
        marker_key: String,
        source_ref: String,
        target_ref: String,
        registry_host: String,
        auth: Option<RegistryAuthResolved>,
    },
    VolumeRemove {
        name: String,
    },
    NetworkRemove {
        id: String,
    },
}

#[derive(Clone, Debug)]
pub(in crate::ui) struct ShellConfirm {
    pub(in crate::ui) label: String,
    pub(in crate::ui) cmdline: String,
}

#[derive(Clone, Debug)]
pub(in crate::ui) struct Connection {
    pub(in crate::ui) runner: Runner,
    pub(in crate::ui) docker: DockerCfg,
}
