extern crate test_utilities;

use self::test_utilities::cloudflare::dns_provider_cloudflare;
use self::test_utilities::utilities::generate_id;
use qovery_engine::models::{
    Action, Clone2, Context, CustomDomain, Environment, EnvironmentAction,
    Storage, StorageType,
};
use qovery_engine::transaction::{DeploymentOption, TransactionResult};
use test_utilities::utilities::{init, is_pod_restarted_aws_env};
use self::test_utilities::aws::context;
use tracing::{span,Level};
// insert how many actions you will use in tests
// args are function you want to use and how many context you want to have
// it permit you to create several different workspaces for each steps
// TODO implement it well
pub fn generate_contexts_and_environments(
    number: u8,
    func: fn(&Context) -> Environment,
) -> (Vec<Context>, Vec<Environment>) {
    let mut context_vec: Vec<Context> = Vec::new();
    let mut env_vec: Vec<Environment> = Vec::new();
    let context = context();
    for _ in std::iter::repeat(number) {
        context_vec.push(context.clone_not_same_execution_id());
        let environment = func(&context);
        env_vec.push(environment);
    }
    (context_vec, env_vec)
}

pub fn deploy_environment(
    context: &Context,
    environment_action: &EnvironmentAction,
) -> TransactionResult {
    let engine = test_utilities::aws::docker_ecr_aws_engine(&context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let cp = test_utilities::aws::cloud_provider_aws(&context);
    let nodes = test_utilities::aws::aws_kubernetes_nodes();
    let dns_provider = dns_provider_cloudflare(context);
    let k = test_utilities::aws::aws_kubernetes_eks(&context, &cp, &dns_provider, nodes);

    tx.deploy_environment_with_options(
        &k,
        &environment_action,
        DeploymentOption {
            force_build: true,
            force_push: true,
        },
    );

    tx.commit()
}

// pub fn pause_environment(
//     context: &Context,
//     environment_action: &EnvironmentAction,
// ) -> TransactionResult {
//     let engine = test_utilities::aws::docker_ecr_aws_engine(&context);
//     let session = engine.session().unwrap();
//     let mut tx = session.transaction();
//
//     let cp = test_utilities::aws::cloud_provider_aws(&context);
//     let nodes = test_utilities::aws::aws_kubernetes_nodes();
//     let dns_provider = dns_provider_cloudflare(context);
//     let k = test_utilities::aws::aws_kubernetes_eks(&context, &cp, &dns_provider, nodes);
//
//     tx.pause_environment(&k, &environment_action);
//
//     tx.commit()
// }

pub fn delete_environment(
    context: &Context,
    environment_action: &EnvironmentAction,
) -> TransactionResult {
    let engine = test_utilities::aws::docker_ecr_aws_engine(&context);
    let session = engine.session().unwrap();
    let mut tx = session.transaction();

    let cp = test_utilities::aws::cloud_provider_aws(&context);
    let nodes = test_utilities::aws::aws_kubernetes_nodes();
    let dns_provider = dns_provider_cloudflare(context);
    let k = test_utilities::aws::aws_kubernetes_eks(&context, &cp, &dns_provider, nodes);

    tx.delete_environment(&k, &environment_action);

    tx.commit()
}

#[test]
fn deploy_a_working_environment_with_no_router_on_aws_eks() {
    init();

    let span = span!(Level::INFO, "deploy_a_working_environment_with_no_router_on_aws_eks");
    let _enter = span.enter();

    let context = context();
    let context_for_delete = context.clone_not_same_execution_id();
    let mut environment = test_utilities::aws::working_minimal_environment(&context);
    let mut environment_for_delete = test_utilities::aws::working_minimal_environment(&context);
    environment.routers = vec![];
    environment_for_delete.routers = vec![];
    environment_for_delete.action = Action::Delete;
    let ea = EnvironmentAction::Environment(environment);
    let ea_delete = EnvironmentAction::Environment(environment_for_delete);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    match delete_environment(&context_for_delete, &ea_delete) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
fn deploy_dockerfile_not_exist() {
    init();

    let span = span!(Level::TRACE, "deploy_dockerfile_not_exist");
    let _enter = span.enter();

    let context = context();
    let context2 = context.clone_not_same_execution_id();

    let environment = test_utilities::aws::working_minimal_environment(&context);
    // working env
    let mut not_working_env = test_utilities::aws::working_minimal_environment(&context2);

    not_working_env.applications = environment
        .applications
        .into_iter()
        .map(|mut app| {
            app.git_url = "https://github.com/Qovery/engine-testing.git".to_string();
            app.branch = "dockerfile-not-exist".to_string();
            app.commit_id = "5cd900a07a17c7aa3c14cb5cb82c62e19219d57c".to_string();
            app.environment_variables = vec![];
            app.dockerfile_path = "".to_string();
            app
        })
        .collect::<Vec<qovery_engine::models::Application>>();

    let ea = EnvironmentAction::Environment(not_working_env);

    match deploy_environment(&context2, &ea) {
        TransactionResult::Ok => assert!(false),
        TransactionResult::Rollback(_) => assert!(true),
        TransactionResult::UnrecoverableError(_, _) => assert!(true),
    };
}

#[test]
fn deploy_a_not_working_environment_with_no_router_on_aws_eks() {
    init();

    let span = span!(Level::TRACE, "deploy_a_not_working_environment_with_no_router_on_aws_eks");
    let _enter = span.enter();

    let context = context();
    let context_for_deletion = context.clone_not_same_execution_id();
    let mut environment = test_utilities::aws::non_working_environment(&context);

    environment.routers = vec![];

    let mut environment_delete =
        test_utilities::aws::non_working_environment(&context_for_deletion);
    environment_delete.routers = vec![];
    environment_delete.action = Action::Delete;
    let ea = EnvironmentAction::Environment(environment);
    let ea_delete = EnvironmentAction::Environment(environment_delete);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(false),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(true),
    };

    match delete_environment(&context_for_deletion, &ea_delete) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(true),
    };

    //Todo: remove the namespace (or project)
}

#[test]
#[ignore]
fn deploy_a_working_environment_with_domain() {
    init();

    let span = span!(Level::TRACE, "deploy_a_working_environment_with_domain");
    let _enter = span.enter();

    let context = context();
    let context_for_deletion = context.clone_not_same_execution_id();

    let environment = test_utilities::aws::working_minimal_environment(&context);

    let mut environment_delete = environment.clone();
    environment_delete.action = Action::Delete;
    let ea = EnvironmentAction::Environment(environment);
    let ea_delete = EnvironmentAction::Environment(environment_delete);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    match delete_environment(&context_for_deletion, &ea_delete) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
#[ignore]
fn deploy_a_working_environment_with_custom_domain() {
    init();

    let span = span!(Level::TRACE, "deploy_a_working_environment_with_custom_domain");
    let _enter = span.enter();

    let context = context();
    let context_for_delete = context.clone_not_same_execution_id();

    let mut environment = test_utilities::aws::working_minimal_environment(&context);
    // Todo: fix domains
    environment.routers = environment
        .routers
        .into_iter()
        .map(|mut router| {
            router.custom_domains = vec![CustomDomain {
                // should be the client domain
                domain: "test-domain.qvy.io".to_string(),
                // should be our domain
                target_domain: "target-domain.oom.sh".to_string(),
            }];
            router
        })
        .collect::<Vec<qovery_engine::models::Router>>();

    let mut environment_delete =
        test_utilities::aws::working_minimal_environment(&context_for_delete);
    environment_delete.routers = environment_delete
        .routers
        .into_iter()
        .map(|mut router| {
            router.custom_domains = vec![CustomDomain {
                // should be the client domain
                domain: "test-domain.qvy.io".to_string(),
                // should be our domain
                target_domain: "target-domain.DEFAULT_TEST_DOMAIN".to_string(),
            }];
            router
        })
        .collect::<Vec<qovery_engine::models::Router>>();
    environment_delete.action = Action::Delete;
    let ea = EnvironmentAction::Environment(environment);
    let ea_delete = EnvironmentAction::Environment(environment_delete);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
    match delete_environment(&context_for_delete, &ea_delete) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
#[ignore]
fn deploy_a_working_environment_with_storage_on_aws_eks() {
    init();

    let span = span!(Level::TRACE, "deploy_a_working_environment_with_storage_on_aws_eks");
    let _enter = span.enter();

    let context = context();
    let context_for_deletion = context.clone_not_same_execution_id();

    let mut environment = test_utilities::aws::working_minimal_environment(&context);

    // Todo: make an image that check there is a mounted disk
    environment.applications = environment
        .applications
        .into_iter()
        .map(|mut app| {
            app.storage = vec![Storage {
                id: generate_id(),
                name: "photos".to_string(),
                storage_type: StorageType::Ssd,
                size_in_gib: 10,
                mount_point: "/mnt/photos".to_string(),
                snapshot_retention_in_days: 0,
            }];
            app
        })
        .collect::<Vec<qovery_engine::models::Application>>();

    let mut environment_delete =
        test_utilities::aws::working_minimal_environment(&context_for_deletion);
    environment_delete.action = Action::Delete;
    environment_delete.applications = environment_delete
        .applications
        .into_iter()
        .map(|mut app| {
            app.storage = vec![Storage {
                id: generate_id(),
                name: "photos".to_string(),
                storage_type: StorageType::Ssd,
                size_in_gib: 10,
                mount_point: "/mnt/photos".to_string(),
                snapshot_retention_in_days: 0,
            }];
            app
        })
        .collect::<Vec<qovery_engine::models::Application>>();
    let ea = EnvironmentAction::Environment(environment);
    let ea_delete = EnvironmentAction::Environment(environment_delete);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    // todo: check the disk is here and with correct size

    match delete_environment(&context_for_deletion, &ea_delete) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    //Todo: remove the namespace (or project)
}

// to check if app redeploy or not, it shouldn't
#[test]
fn redeploy_same_app_with_ebs() {
    init();

    let span = span!(Level::TRACE, "redeploy_same_app_with_ebs");
    let _enter = span.enter();

    let context = context();
    let context_bis = context.clone_not_same_execution_id();
    let context_for_deletion = context.clone_not_same_execution_id();

    let mut environment = test_utilities::aws::working_minimal_environment(&context);

    // Todo: make an image that check there is a mounted disk
    environment.applications = environment
        .applications
        .into_iter()
        .map(|mut app| {
            app.storage = vec![Storage {
                id: generate_id(),
                name: "photos".to_string(),
                storage_type: StorageType::Ssd,
                size_in_gib: 10,
                mount_point: "/mnt/photos".to_string(),
                snapshot_retention_in_days: 0,
            }];
            app
        })
        .collect::<Vec<qovery_engine::models::Application>>();
    let environment_redeploy = environment.clone();
    let environment_check1 = environment.clone();
    let environment_check2 = environment.clone();
    let mut environment_delete = environment.clone();
    environment_delete.action = Action::Delete;

    let ea = EnvironmentAction::Environment(environment);
    let ea2 = EnvironmentAction::Environment(environment_redeploy);
    let ea_delete = EnvironmentAction::Environment(environment_delete);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
    let app_name = format!("{}-0", &environment_check1.applications[0].name);
    let (_, number) = is_pod_restarted_aws_env(environment_check1, app_name.clone().as_str());

    match deploy_environment(&context_bis, &ea2) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    let (_, number2) = is_pod_restarted_aws_env(environment_check2, app_name.as_str());
    //nothing change in the app, so, it shouldn't be restarted
    assert!(number.eq(&number2));
    match delete_environment(&context_for_deletion, &ea_delete) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

// #[test]
// fn deploy_a_working_environment_with_external_service() {
//     init();
//
//     let context = context();
//     let deletion_context = context.clone_not_same_execution_id();
//
//     let mut environment = test_utilities::aws::working_minimal_environment(&context);
//
//     // no apps
//     environment.applications = vec![];
//
//     environment.external_services = vec![ExternalService {
//         id: generate_id(),
//         action: Action::Create,
//         name: "my-external-service".to_string(),
//         total_cpus: "500m".to_string(),
//         total_ram_in_mib: 512,
//         git_url: "https://github.com/evoxmusic/qovery-external-service-example.git".to_string(),
//         git_credentials: GitCredentials {
//             login: "x-access-token".to_string(),
//             access_token: "CHANGE ME".to_string(), // fake one
//             expired_at: Utc::now(),
//         },
//         branch: "master".to_string(),
//         commit_id: "db322f2f4ac70933f16e8a422ea9f72e1e14df22".to_string(),
//         on_create_dockerfile_path: "extsvc/Dockerfile.on-create".to_string(),
//         on_pause_dockerfile_path: "extsvc/Dockerfile.on-pause".to_string(),
//         on_delete_dockerfile_path: "extsvc/Dockerfile.on-delete".to_string(),
//         environment_variables: vec![],
//     }];
//
//     let mut environment_delete = environment.clone();
//     environment_delete.action = Action::Delete;
//
//     let ea = EnvironmentAction::Environment(environment);
//     let ea_delete = EnvironmentAction::Environment(environment_delete);
//
//     match deploy_environment(&context, &ea) {
//         TransactionResult::Ok => assert!(true),
//         TransactionResult::Rollback(_) => assert!(false),
//         TransactionResult::UnrecoverableError(_, _) => assert!(false),
//     };
//
//     match delete_environment(&deletion_context, &ea_delete) {
//         TransactionResult::Ok => assert!(true),
//         TransactionResult::Rollback(_) => assert!(false),
//         TransactionResult::UnrecoverableError(_, _) => assert!(false),
//     };
//
//     // TODO: remove the namespace (or project)
// }

/*#[test]
#[ignore]
fn deploy_a_working_production_environment_with_all_options_on_aws_eks() {
    init();

    let context = context();

    let mut environment = test_utilities::aws::working_environment(&context);
    environment.kind = Kind::Production;
    let environment_delete = environment.clone_not_same_execution_id();
    let ea = EnvironmentAction::Environment(environment);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
    let ea_delete = EnvironmentAction::Environment(environment_delete);
    match delete_environment(&context, &ea_delete) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}*/

#[test]
fn deploy_a_not_working_environment_and_after_working_environment() {
    init();

    let span = span!(Level::TRACE, "deploy_a_not_working_environment_and_after_working_environment");
    let _enter = span.enter();

    // let mut contex_envs = generate_contexts_and_environments(3, test_utilities::aws::working_minimal_environment);
    let context = context();
    let context_for_not_working = context.clone_not_same_execution_id();
    let context_for_delete = context.clone_not_same_execution_id();
    // env part generation
    let environment = test_utilities::aws::working_minimal_environment(&context);
    let mut environment_for_not_working =
        test_utilities::aws::working_minimal_environment(&context_for_not_working);
    // this environment is broken by container exit
    environment_for_not_working.applications = environment_for_not_working
        .applications
        .into_iter()
        .map(|mut app| {
            app.git_url = "https://gitlab.com/maathor/my-exit-container".to_string();
            app.branch = "master".to_string();
            app.commit_id = "55bc95a23fbf91a7699c28c5f61722d4f48201c9".to_string();
            app.environment_variables = vec![];
            app
        })
        .collect::<Vec<qovery_engine::models::Application>>();

    let mut environment_for_delete =
        test_utilities::aws::working_minimal_environment(&context_for_delete);
    environment_for_delete.action = Action::Delete;
    // environment actions
    let ea = EnvironmentAction::Environment(environment);
    let ea_not_working = EnvironmentAction::Environment(environment_for_not_working);
    let ea_delete = EnvironmentAction::Environment(environment_for_delete);

    match deploy_environment(&context_for_not_working, &ea_not_working) {
        TransactionResult::Ok => assert!(false),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(true),
    };
    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
    match delete_environment(&context_for_delete, &ea_delete) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
fn deploy_ok_fail_fail_ok_environment() {
    init();

    let span = span!(Level::TRACE, "deploy_ok_fail_fail_ok_environment");
    let _enter = span.enter();

    // working env
    let context = context();
    let environment = test_utilities::aws::working_minimal_environment(&context);
    let ea = EnvironmentAction::Environment(environment);
    // not working 1
    let context_for_not_working = context.clone_not_same_execution_id();
    let mut not_working_env =
        test_utilities::aws::working_minimal_environment(&context_for_not_working);
    // not working 2
    let context_for_not_working2 = context.clone_not_same_execution_id();
    let mut not_working_env2 =
        test_utilities::aws::working_minimal_environment(&context_for_not_working2);
    // final env is working
    let context_for_working2 = context.clone_not_same_execution_id();
    let working_env_2 = test_utilities::aws::working_minimal_environment(&context_for_working2);
    let ea2 = EnvironmentAction::Environment(working_env_2);
    // work for delete
    let context_for_delete = context.clone_not_same_execution_id();
    let mut delete_env = test_utilities::aws::working_minimal_environment(&context_for_delete);
    delete_env.action = Action::Delete;
    let ea_delete = EnvironmentAction::Environment(delete_env);
    // override application to make envs to be not working
    not_working_env.applications = not_working_env
        .applications
        .into_iter()
        .map(|mut app| {
            app.git_url = "https://gitlab.com/maathor/my-exit-container".to_string();
            app.branch = "master".to_string();
            app.commit_id = "55bc95a23fbf91a7699c28c5f61722d4f48201c9".to_string();
            app.environment_variables = vec![];
            app
        })
        .collect::<Vec<qovery_engine::models::Application>>();
    not_working_env2.applications = not_working_env2
        .applications
        .into_iter()
        .map(|mut app| {
            app.git_url = "https://gitlab.com/maathor/my-exit-container".to_string();
            app.branch = "master".to_string();
            app.commit_id = "55bc95a23fbf91a7699c28c5f61722d4f48201c9".to_string();
            app.environment_variables = vec![];
            app
        })
        .collect::<Vec<qovery_engine::models::Application>>();

    let ea_not_working = EnvironmentAction::Environment(not_working_env);
    let ea_not_working2 = EnvironmentAction::Environment(not_working_env2);

    // OK
    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
    // FAIL and rollback
    match deploy_environment(&context_for_not_working, &ea_not_working) {
        TransactionResult::Ok => assert!(false),
        TransactionResult::Rollback(_) => assert!(true),
        TransactionResult::UnrecoverableError(_, _) => assert!(true),
    };
    // FAIL and Rollback again
    match deploy_environment(&context_for_not_working2, &ea_not_working2) {
        TransactionResult::Ok => assert!(false),
        TransactionResult::Rollback(_) => assert!(true),
        TransactionResult::UnrecoverableError(_, _) => assert!(true),
    };
    // Should be working
    match deploy_environment(&context_for_working2, &ea2) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
    match delete_environment(&context_for_delete, &ea_delete) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
fn deploy_a_non_working_environment_with_no_failover_on_aws_eks() {
    init();

    let span = span!(Level::TRACE, "deploy_a_non_working_environment_with_no_failover_on_aws_eks");
    let _enter = span.enter();

    let context = context();

    let environment = test_utilities::aws::non_working_environment(&context);

    let ea = EnvironmentAction::Environment(environment);

    let context_for_delete = context.clone_not_same_execution_id();
    let mut delete_env = test_utilities::aws::non_working_environment(&context_for_delete);
    delete_env.action = Action::Delete;
    let ea_delete = EnvironmentAction::Environment(delete_env);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(false),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(true),
    };
    match delete_environment(&context_for_delete, &ea_delete) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
#[ignore]
fn deploy_a_non_working_environment_with_a_working_failover_on_aws_eks() {
    init();

    let span = span!(Level::TRACE, "deploy_a_non_working_environment_with_a_working_failover_on_aws_eks");
    let _enter = span.enter();

    // context for non working environment
    let context = context();

    let environment = test_utilities::aws::non_working_environment(&context);
    let failover_environment = test_utilities::aws::working_minimal_environment(&context);
    // context for deletion
    let context_deletion = context.clone_not_same_execution_id();
    let mut delete_env = test_utilities::aws::working_minimal_environment(&context_deletion);
    delete_env.action = Action::Delete;
    let ea_delete = EnvironmentAction::Environment(delete_env);
    let ea = EnvironmentAction::EnvironmentWithFailover(environment, failover_environment);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(false),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(true),
    };
    match delete_environment(&context_deletion, &ea_delete) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

// fn deploy_2_non_working_environments_with_2_working_failovers_on_aws_eks() {
//     init();
//     // context for non working environment
//     let context_failover_1 = context();
//     let context_failover_2 = context_failover_1.clone_not_same_execution_id();
//
//     let context_first_fail_deployement_1 = context_failover_1.clone_not_same_execution_id();
//     let context_second_fail_deployement_2 = context_failover_1.clone_not_same_execution_id();
//
//     let mut failover_environment_1 = test_utilities::aws::echo_app_environment(&context_failover_1);
//     let mut fail_app_1 =
//         test_utilities::aws::non_working_environment(&context_first_fail_deployement_1);
//     let mut failover_environment_2 = test_utilities::aws::echo_app_environment(&context_failover_2);
//     let mut fail_app_2 =
//         test_utilities::aws::non_working_environment(&context_second_fail_deployement_2);
//
//     failover_environment_2.applications = failover_environment_2
//         .applications
//         .into_iter()
//         .map(|mut app| {
//             app.environment_variables = vec![EnvironmentVariable {
//                 key: "ECHO_TEXT".to_string(),
//                 value: "Lilou".to_string(),
//             }];
//             app
//         })
//         .collect::<Vec<qovery_engine::models::Application>>();
//
//     // context for deletion
//     let context_deletion = context_failover_1.clone_not_same_execution_id();
//     let mut delete_env = test_utilities::aws::echo_app_environment(&context_deletion);
//     delete_env.action = Action::Delete;
//     let ea_delete = EnvironmentAction::Environment(delete_env);
//
//     // first deployement
//     let ea1 = EnvironmentAction::EnvironmentWithFailover(fail_app_1, failover_environment_1);
//     let ea2 = EnvironmentAction::EnvironmentWithFailover(fail_app_2, failover_environment_2);
//
//     match deploy_environment(&context_failover_1, &ea1) {
//         TransactionResult::Ok => assert!(false),
//         TransactionResult::Rollback(_) => assert!(false),
//         TransactionResult::UnrecoverableError(_, _) => assert!(true),
//     };
//
//     match deploy_environment(&context_failover_2, &ea2) {
//         TransactionResult::Ok => assert!(false),
//         TransactionResult::Rollback(_) => assert!(false),
//         TransactionResult::UnrecoverableError(_, _) => assert!(true),
//     };
//
//     match delete_environment(&context_deletion, &ea_delete) {
//         TransactionResult::Ok => assert!(true),
//         TransactionResult::Rollback(_) => assert!(false),
//         TransactionResult::UnrecoverableError(_, _) => assert!(false),
//     };
// }

#[test]
#[ignore]
fn deploy_a_non_working_environment_with_a_non_working_failover_on_aws_eks() {
    init();

    let span = span!(Level::TRACE, "deploy_a_non_working_environment_with_a_non_working_failover_on_aws_eks");
    let _enter = span.enter();

    let context = context();

    let environment = test_utilities::aws::non_working_environment(&context);
    let failover_environment = test_utilities::aws::non_working_environment(&context);

    let context_for_deletion = context.clone_not_same_execution_id();
    let mut delete_env = test_utilities::aws::non_working_environment(&context_for_deletion);
    delete_env.action = Action::Delete;
    // environment action initialize
    let ea_delete = EnvironmentAction::Environment(delete_env);
    let ea = EnvironmentAction::EnvironmentWithFailover(environment, failover_environment);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(false),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(true),
    };
    match delete_environment(&context_for_deletion, &ea_delete) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(true),
    };
}

/*#[test]
#[ignore]
fn deploy_a_working_environment_with_a_failing_default_domain_on_aws_eks() {
    init();

    // TODO
}

#[test]
#[ignore]
fn deploy_but_fail_to_push_image_on_container_registry() {
    init();

    // TODO
}*/
/*
fn pause_a_working_development_environment_on_aws_eks() {
    init();

    let context = test_utilities::aws::context();

    let mut environment = test_utilities::aws::working_environment(&context);
    environment.kind = Kind::Development;

    let ea = EnvironmentAction::Environment(environment);

    match pause_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
#[ignore]
fn pause_a_working_production_environment_on_aws_eks() {
    init();

    let context = test_utilities::aws::context();

    let mut environment = test_utilities::aws::working_environment(&context);
    environment.kind = Kind::Production;

    let ea = EnvironmentAction::Environment(environment);

    match pause_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
#[ignore]
fn pause_a_non_working_environment_on_aws_eks() {
    init();

    let context = test_utilities::aws::context();

    let mut environment = test_utilities::aws::non_working_environment(&context);

    let ea = EnvironmentAction::Environment(environment);

    match pause_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}

#[test]
#[ignore]
fn start_and_pause_and_start_and_delete_a_working_environment_on_aws_eks() {
    init();

    // START
    let context = test_utilities::aws::context();

    let mut environment = test_utilities::aws::working_environment(&context);
    let ea = EnvironmentAction::Environment(environment);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    // PAUSE
    let context = test_utilities::aws::context();

    let mut environment = test_utilities::aws::working_environment(&context);
    let ea = EnvironmentAction::Environment(environment);

    match pause_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    // START
    let context = test_utilities::aws::context();

    let mut environment = test_utilities::aws::working_environment(&context);
    let ea = EnvironmentAction::Environment(environment);

    match deploy_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };

    // DELETE
    let context = test_utilities::aws::context();

    let mut environment = test_utilities::aws::working_environment(&context);
    let ea = EnvironmentAction::Environment(environment);

    match delete_environment(&context, &ea) {
        TransactionResult::Ok => assert!(true),
        TransactionResult::Rollback(_) => assert!(false),
        TransactionResult::UnrecoverableError(_, _) => assert!(false),
    };
}
*/
