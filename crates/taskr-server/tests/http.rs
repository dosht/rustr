//! End to end over real HTTP: taskr-client and the shared AppController on one
//! side, taskr-server with the same use cases on the other. Runs on in-memory
//! storage always, and on Postgres when TASKR_TEST_DATABASE_URL is set.

use std::sync::{Arc, Mutex};

use taskr_client::{DevAuth, HttpApi};
use taskr_core::session::app::{Session, SessionStore};
use taskr_core::session::presentation::{
    AppController, AppIntent, AppVm, Screen, ScreenVm, SignedInVm,
};
use taskr_core::shared::access::{AccessError, Action, Role, WorkspaceId};
use taskr_core::shared::telemetry::NoTelemetry;
use taskr_core::tasks::app::{TaskListRepository, TasksApi, TasksError};
use taskr_core::tasks::domain::TaskError;
use taskr_core::tasks::presentation::Intent;
use taskr_core::testing::{FixedClock, InMemoryTaskListRepository, InMemoryWorkspaceRepository};
use taskr_core::workspaces::app::{WorkspaceRepository, WorkspacesApi, WorkspacesError};
use taskr_core::workspaces::domain::WorkspaceError;
use taskr_core::workspaces::presentation::MembersIntent;
use taskr_server::identity::{DevIdentity, Identities};
use taskr_server::{Server, router};

#[derive(Default)]
struct MemoryStore(Mutex<Option<Session>>);

impl SessionStore for MemoryStore {
    fn load(&self) -> Option<Session> {
        self.0.lock().unwrap().clone()
    }
    fn save(&self, s: &Session) {
        *self.0.lock().unwrap() = Some(s.clone());
    }
    fn clear(&self) {
        *self.0.lock().unwrap() = None;
    }
}

async fn start(
    task_lists: Arc<dyn TaskListRepository>,
    workspaces: Arc<dyn WorkspaceRepository>,
) -> String {
    let server = Server {
        task_lists,
        workspaces,
        identity: Arc::new(Identities { dev: Some(Arc::new(DevIdentity)), real: None }),
        clock: Arc::new(FixedClock::at(1_000)),
        telemetry: Arc::new(NoTelemetry),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router(server)).await.unwrap() });
    format!("http://{addr}/api")
}

struct Person {
    app: AppController,
    api: Arc<HttpApi>,
    store: Arc<MemoryStore>,
}

fn person(base: &str) -> Person {
    let store = Arc::new(MemoryStore::default());
    let api = Arc::new(HttpApi::new(base, store.clone()));
    let app = AppController::new(
        Arc::new(DevAuth),
        store.clone(),
        api.clone(),
        api.clone(),
        Arc::new(FixedClock::at(1_000)),
    );
    Person { app, api, store }
}

async fn sign_in(p: &Person, email: &str) -> SignedInVm {
    p.app.dispatch(AppIntent::Started).await;
    match p
        .app
        .dispatch(AppIntent::SignInSubmitted { email: email.into(), password: "x".into() })
        .await
    {
        AppVm::SignedIn(vm) => vm,
        other => panic!("{other:?}"),
    }
}

fn unique(name: &str) -> String {
    format!("{name}-{}@example.com", uuid::Uuid::new_v4().simple())
}

async fn scenario(base: String) {
    let ada_email = unique("ada");
    let bob_email = unique("bob");
    let ada = person(&base);
    let vm = sign_in(&ada, &ada_email).await;
    assert!(vm.title.ends_with("'s workspace · Tasks"), "{}", vm.title);
    let ws = vm.workspace.clone().expect("a workspace is open");
    let add = |title: &str| AppIntent::Tasks(ws.clone(), Intent::AddSubmitted(title.into()));

    let vm = ada.app.dispatch(add("Ship M1")).await;
    let AppVm::SignedIn(SignedInVm { screen: ScreenVm::Tasks(t), .. }) = vm else {
        panic!("{vm:?}")
    };
    assert_eq!(t.rows[0].title, "Ship M1");
    assert!(t.can_change);

    let vm = ada.app.dispatch(add("ship m1")).await;
    let AppVm::SignedIn(SignedInVm { screen: ScreenVm::Tasks(t), .. }) = vm else { panic!() };
    assert_eq!(
        t.error.as_deref(),
        Some("There is already an open task with that title."),
        "a domain rejection crosses HTTP and reads the same"
    );

    ada.app.dispatch(AppIntent::ScreenSelected(Screen::Members)).await;
    let vm = ada
        .app
        .dispatch(AppIntent::Members(
            ws.clone(),
            MembersIntent::AddSubmitted { email: bob_email.clone(), role: Role::Viewer },
        ))
        .await;
    let AppVm::SignedIn(SignedInVm { screen: ScreenVm::Members(m), .. }) = vm else {
        panic!("{vm:?}")
    };
    assert_eq!(m.rows.len(), 2);
    assert!(m.error.is_none(), "{:?}", m.error);

    assert_eq!(
        ada.api.remove_member(&ws, ada_email.clone()).await,
        Err(WorkspacesError::Rejected(WorkspaceError::LastOwner))
    );

    let bob = person(&base);
    let vm = sign_in(&bob, &bob_email).await;
    assert_eq!(vm.workspaces.len(), 2, "invited, and still a workspace of their own");
    assert_eq!(vm.workspaces[0].role_label, "Viewer", "ada's workspace sorts first");
    assert_eq!(vm.workspaces[1].role_label, "Owner");
    let ScreenVm::Tasks(t) = vm.screen else { panic!() };
    assert_eq!(t.rows[0].title, "Ship M1", "bob sees ada's tasks");
    assert!(!t.can_change);

    assert_eq!(
        bob.api.add(&ws, "sneaky".into()).await,
        Err(TasksError::Access(AccessError::Forbidden(Action::ChangeTasks))),
        "the server enforces the role even if a client ignores can_change"
    );
    assert_eq!(
        bob.api.set_member(&ws, bob_email.clone(), Role::Owner).await,
        Err(WorkspacesError::Access(AccessError::Forbidden(Action::ManageMembers)))
    );
    assert_eq!(
        bob.api.complete(&ws, 99).await,
        Err(TasksError::Access(AccessError::Forbidden(Action::ChangeTasks)))
    );

    let eve = person(&base);
    sign_in(&eve, &unique("eve")).await;
    assert_eq!(
        eve.api.list(&ws).await,
        Err(TasksError::Access(AccessError::NotAMember)),
        "strangers cannot read a workspace"
    );
    assert_eq!(
        eve.api.list(&WorkspaceId("no-such-workspace".into())).await,
        Err(TasksError::Access(AccessError::NotAMember))
    );

    ada.app.dispatch(AppIntent::ScreenSelected(Screen::Tasks)).await;
    assert_eq!(
        ada.api.complete(&ws, 42).await,
        Err(TasksError::Rejected(TaskError::NotFound(taskr_core::tasks::domain::TaskId(42))))
    );

    ada.store.save(&Session {
        access_token: "garbage".into(),
        refresh_token: None,
        email: ada_email,
        expires_at: None,
    });
    match ada.app.dispatch(AppIntent::Tasks(ws.clone(), Intent::Refreshed)).await {
        AppVm::SignedOut(vm) => {
            assert_eq!(vm.error.as_deref(), Some("Your session has ended. Sign in again."))
        }
        other => panic!("an invalid token signs the person out, got {other:?}"),
    }
    assert!(ada.store.load().is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn full_scenario_in_memory() {
    let base = start(
        Arc::new(InMemoryTaskListRepository::default()),
        Arc::new(InMemoryWorkspaceRepository::default()),
    )
    .await;
    scenario(base).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn full_scenario_on_postgres() {
    let Ok(url) = std::env::var("TASKR_TEST_DATABASE_URL") else {
        eprintln!("skipped: TASKR_TEST_DATABASE_URL is not set");
        return;
    };
    let pool = taskr_postgres::connect(&url).await.unwrap();
    let base = start(
        Arc::new(taskr_postgres::PgTaskListRepository::new(pool.clone())),
        Arc::new(taskr_postgres::PgWorkspaceRepository::new(pool)),
    )
    .await;
    scenario(base).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn errors_are_json_with_codes() {
    let base = start(
        Arc::new(InMemoryTaskListRepository::default()),
        Arc::new(InMemoryWorkspaceRepository::default()),
    )
    .await;
    let http = reqwest::Client::new();
    let res = http.get(format!("{base}/me")).send().await.unwrap();
    assert_eq!(res.status(), 401);
    assert!(res.headers().contains_key("x-request-id"));
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["error"]["code"], "unauthenticated");

    let res = http
        .post(format!("{base}/workspaces/x/tasks"))
        .bearer_auth("dev:a@b.co")
        .header("content-type", "application/json")
        .body("{not json")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 400);
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body["error"]["code"], "bad_request");

    let res = http.get(format!("{base}/health")).send().await.unwrap();
    assert_eq!(res.status(), 200);
}
