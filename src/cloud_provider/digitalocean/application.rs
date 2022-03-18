use tera::Context as TeraContext;

use crate::build_platform::Image;
use crate::cloud_provider::kubernetes::validate_k8s_required_cpu_and_burstable;
use crate::cloud_provider::models::{
    EnvironmentVariable, EnvironmentVariableDataTemplate, Storage, StorageDataTemplate,
};
use crate::cloud_provider::service::{
    default_tera_context, delete_stateless_service, deploy_stateless_service_error, deploy_user_stateless_service,
    scale_down_application, send_progress_on_long_task, Action, Create, Delete, Helm, Pause, Service, ServiceType,
    StatelessService,
};
use crate::cloud_provider::utilities::{print_action, sanitize_name};
use crate::cloud_provider::DeploymentTarget;
use crate::cmd::helm::Timeout;
use crate::cmd::kubectl::ScalingKind::{Deployment, Statefulset};
use crate::errors::{CommandError, EngineError};
use crate::events::{EnvironmentStep, Stage, ToTransmitter, Transmitter};
use crate::logger::Logger;
use crate::models::{Context, Listen, Listener, Listeners, ListenersHelper, Port};
use ::function_name::named;
use std::fmt;
use std::str::FromStr;

pub struct Application {
    context: Context,
    id: String,
    action: Action,
    name: String,
    ports: Vec<Port>,
    total_cpus: String,
    cpu_burst: String,
    total_ram_in_mib: u32,
    min_instances: u32,
    max_instances: u32,
    start_timeout_in_seconds: u32,
    image: Image,
    storage: Vec<Storage<StorageType>>,
    environment_variables: Vec<EnvironmentVariable>,
    listeners: Listeners,
    logger: Box<dyn Logger>,
}

impl Application {
    pub fn new(
        context: Context,
        id: &str,
        action: Action,
        name: &str,
        ports: Vec<Port>,
        total_cpus: String,
        cpu_burst: String,
        total_ram_in_mib: u32,
        min_instances: u32,
        max_instances: u32,
        start_timeout_in_seconds: u32,
        image: Image,
        storage: Vec<Storage<StorageType>>,
        environment_variables: Vec<EnvironmentVariable>,
        listeners: Listeners,
        logger: Box<dyn Logger>,
    ) -> Self {
        Application {
            context,
            id: id.to_string(),
            action,
            name: name.to_string(),
            ports,
            total_cpus,
            cpu_burst,
            total_ram_in_mib,
            min_instances,
            max_instances,
            start_timeout_in_seconds,
            image,
            storage,
            environment_variables,
            listeners,
            logger,
        }
    }

    fn is_stateful(&self) -> bool {
        self.storage.len() > 0
    }

    fn cloud_provider_name(&self) -> &str {
        "digitalocean"
    }

    fn struct_name(&self) -> &str {
        "application"
    }
}

impl crate::cloud_provider::service::Application for Application {
    fn image(&self) -> &Image {
        &self.image
    }

    fn set_image(&mut self, image: Image) {
        self.image = image;
    }
}

impl Helm for Application {
    fn helm_selector(&self) -> Option<String> {
        self.selector()
    }

    fn helm_release_name(&self) -> String {
        crate::string::cut(format!("application-{}-{}", self.name, self.id), 50)
    }

    fn helm_chart_dir(&self) -> String {
        format!("{}/digitalocean/charts/q-application", self.context.lib_root_dir())
    }

    fn helm_chart_values_dir(&self) -> String {
        String::new()
    }

    fn helm_chart_external_name_service_dir(&self) -> String {
        String::new()
    }
}

impl StatelessService for Application {}

impl ToTransmitter for Application {
    fn to_transmitter(&self) -> Transmitter {
        Transmitter::Application(self.id().to_string(), self.name().to_string())
    }
}

impl Service for Application {
    fn context(&self) -> &Context {
        &self.context
    }

    fn service_type(&self) -> ServiceType {
        ServiceType::Application
    }

    fn id(&self) -> &str {
        self.id.as_str()
    }

    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn sanitized_name(&self) -> String {
        sanitize_name("app", self.name())
    }

    fn version(&self) -> String {
        self.image.commit_id.clone()
    }

    fn action(&self) -> &Action {
        &self.action
    }

    fn private_port(&self) -> Option<u16> {
        self.ports
            .iter()
            .find(|port| port.publicly_accessible)
            .map(|port| port.port)
    }

    fn start_timeout(&self) -> Timeout<u32> {
        Timeout::Value((self.start_timeout_in_seconds + 10) * 4)
    }

    fn total_cpus(&self) -> String {
        self.total_cpus.to_string()
    }

    fn cpu_burst(&self) -> String {
        self.cpu_burst.to_string()
    }

    fn total_ram_in_mib(&self) -> u32 {
        self.total_ram_in_mib
    }

    fn min_instances(&self) -> u32 {
        self.min_instances
    }

    fn max_instances(&self) -> u32 {
        self.max_instances
    }

    fn publicly_accessible(&self) -> bool {
        self.private_port().is_some()
    }

    fn tera_context(&self, target: &DeploymentTarget) -> Result<TeraContext, EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::LoadConfiguration));
        let kubernetes = target.kubernetes;
        let environment = target.environment;
        let mut context = default_tera_context(self, kubernetes, environment);
        let commit_id = self.image.commit_id.as_str();

        context.insert("helm_app_version", &commit_id[..7]);
        context.insert("image_name_with_tag", &self.image.full_image_name_with_tag());

        let cpu_limits = match validate_k8s_required_cpu_and_burstable(
            &ListenersHelper::new(&self.listeners),
            &self.context.execution_id(),
            &self.id,
            self.total_cpus(),
            self.cpu_burst(),
            event_details.clone(),
            self.logger(),
        ) {
            Ok(l) => l,
            Err(e) => {
                return Err(EngineError::new_k8s_validate_required_cpu_and_burstable_error(
                    event_details.clone(),
                    self.total_cpus(),
                    self.cpu_burst(),
                    e,
                ));
            }
        };
        context.insert("cpu_burst", &cpu_limits.cpu_limit);

        let environment_variables = self
            .environment_variables
            .iter()
            .map(|ev| EnvironmentVariableDataTemplate {
                key: ev.key.clone(),
                value: ev.value.clone(),
            })
            .collect::<Vec<_>>();

        context.insert("environment_variables", &environment_variables);
        context.insert("ports", &self.ports);
        context.insert("is_registry_secret", &true);

        // This is specific to digital ocean as it is them that create the registry secret
        // we don't have the hand on it
        context.insert("registry_secret", &self.image.registry_name);

        let storage = self
            .storage
            .iter()
            .map(|s| StorageDataTemplate {
                id: s.id.clone(),
                name: s.name.clone(),
                storage_type: match s.storage_type {
                    StorageType::Standard => "do-block-storage",
                }
                .to_string(),
                size_in_gib: s.size_in_gib,
                mount_point: s.mount_point.clone(),
                snapshot_retention_in_days: s.snapshot_retention_in_days,
            })
            .collect::<Vec<_>>();

        let is_storage = storage.len() > 0;

        context.insert("storage", &storage);
        context.insert("is_storage", &is_storage);
        context.insert("clone", &false);
        context.insert("start_timeout_in_seconds", &self.start_timeout_in_seconds);

        if self.context.resource_expiration_in_seconds().is_some() {
            context.insert(
                "resource_expiration_in_seconds",
                &self.context.resource_expiration_in_seconds(),
            )
        }

        Ok(context)
    }

    fn logger(&self) -> &dyn Logger {
        &*self.logger
    }

    fn selector(&self) -> Option<String> {
        Some(format!("appId={}", self.id))
    }
}

impl Create for Application {
    #[named]
    fn on_create(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details.clone(),
            self.logger(),
        );

        send_progress_on_long_task(self, crate::cloud_provider::service::Action::Create, || {
            deploy_user_stateless_service(target, self)
        })
    }

    fn on_create_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    #[named]
    fn on_create_error(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Deploy));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );

        send_progress_on_long_task(self, crate::cloud_provider::service::Action::Create, || {
            deploy_stateless_service_error(target, self)
        })
    }
}

impl Pause for Application {
    #[named]
    fn on_pause(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Pause));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );

        send_progress_on_long_task(self, crate::cloud_provider::service::Action::Pause, || {
            scale_down_application(
                target,
                self,
                0,
                if self.is_stateful() { Statefulset } else { Deployment },
            )
        })
    }

    fn on_pause_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    #[named]
    fn on_pause_error(&self, _target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Pause));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details,
            self.logger(),
        );

        Ok(())
    }
}

impl Delete for Application {
    #[named]
    fn on_delete(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Delete));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details.clone(),
            self.logger(),
        );

        send_progress_on_long_task(self, crate::cloud_provider::service::Action::Delete, || {
            delete_stateless_service(target, self, false, event_details.clone())
        })
    }

    fn on_delete_check(&self) -> Result<(), EngineError> {
        Ok(())
    }

    #[named]
    fn on_delete_error(&self, target: &DeploymentTarget) -> Result<(), EngineError> {
        let event_details = self.get_event_details(Stage::Environment(EnvironmentStep::Delete));
        print_action(
            self.cloud_provider_name(),
            self.struct_name(),
            function_name!(),
            self.name(),
            event_details.clone(),
            self.logger(),
        );

        send_progress_on_long_task(self, crate::cloud_provider::service::Action::Delete, || {
            delete_stateless_service(target, self, true, event_details.clone())
        })
    }
}

impl Listen for Application {
    fn listeners(&self) -> &Listeners {
        &self.listeners
    }

    fn add_listener(&mut self, listener: Listener) {
        self.listeners.push(listener);
    }
}

#[derive(Clone, Eq, PartialEq, Hash)]
pub enum StorageType {
    Standard,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum DoRegion {
    NewYorkCity1,
    NewYorkCity2,
    NewYorkCity3,
    Amsterdam2,
    Amsterdam3,
    SanFrancisco1,
    SanFrancisco2,
    SanFrancisco3,
    Singapore,
    London,
    Frankfurt,
    Toronto,
    Bangalore,
}

impl DoRegion {
    pub fn as_str(&self) -> &str {
        match self {
            DoRegion::NewYorkCity1 => "nyc1",
            DoRegion::NewYorkCity2 => "nyc2",
            DoRegion::NewYorkCity3 => "nyc3",
            DoRegion::Amsterdam2 => "ams2",
            DoRegion::Amsterdam3 => "ams3",
            DoRegion::SanFrancisco1 => "sfo1",
            DoRegion::SanFrancisco2 => "sfo2",
            DoRegion::SanFrancisco3 => "sfo3",
            DoRegion::Singapore => "sgp1",
            DoRegion::London => "lon1",
            DoRegion::Frankfurt => "fra1",
            DoRegion::Toronto => "tor1",
            DoRegion::Bangalore => "blr1",
        }
    }
}

impl fmt::Display for DoRegion {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            DoRegion::NewYorkCity1 => write!(f, "nyc1"),
            DoRegion::NewYorkCity2 => write!(f, "nyc2"),
            DoRegion::NewYorkCity3 => write!(f, "nyc3"),
            DoRegion::Amsterdam2 => write!(f, "ams2"),
            DoRegion::Amsterdam3 => write!(f, "ams3"),
            DoRegion::SanFrancisco1 => write!(f, "sfo1"),
            DoRegion::SanFrancisco2 => write!(f, "sfo2"),
            DoRegion::SanFrancisco3 => write!(f, "sfo3"),
            DoRegion::Singapore => write!(f, "sgp1"),
            DoRegion::London => write!(f, "lon1"),
            DoRegion::Frankfurt => write!(f, "fra1"),
            DoRegion::Toronto => write!(f, "tor1"),
            DoRegion::Bangalore => write!(f, "blr1"),
        }
    }
}

impl FromStr for DoRegion {
    type Err = CommandError;

    fn from_str(s: &str) -> Result<DoRegion, CommandError> {
        match s {
            "nyc1" => Ok(DoRegion::NewYorkCity1),
            "nyc2" => Ok(DoRegion::NewYorkCity2),
            "nyc3" => Ok(DoRegion::NewYorkCity3),
            "ams2" => Ok(DoRegion::Amsterdam2),
            "ams3" => Ok(DoRegion::Amsterdam3),
            "sfo1" => Ok(DoRegion::SanFrancisco1),
            "sfo2" => Ok(DoRegion::SanFrancisco2),
            "sfo3" => Ok(DoRegion::SanFrancisco3),
            "sgp1" => Ok(DoRegion::Singapore),
            "lon1" => Ok(DoRegion::London),
            "fra1" => Ok(DoRegion::Frankfurt),
            "tor1" => Ok(DoRegion::Toronto),
            "blr1" => Ok(DoRegion::Bangalore),
            _ => {
                return Err(CommandError::new_from_safe_message(format!(
                    "`{}` region is not supported",
                    s
                )));
            }
        }
    }
}
