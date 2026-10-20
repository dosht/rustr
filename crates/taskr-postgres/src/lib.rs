//! PostgreSQL adapters. Aggregates are stored as rows, loaded in one statement
//! (one consistent snapshot) and saved in one transaction together with their
//! outbox events. The `revision` column is the optimistic concurrency token.

use std::collections::BTreeMap;

use serde_json::{Map, Value};
use sqlx::postgres::{PgPool, PgPoolOptions, PgQueryResult};
use sqlx::types::Json;
use sqlx::{Postgres, Transaction};
use taskr_core::shared::access::{Email, Role, UserId, WorkspaceId};
use taskr_core::shared::events::DomainEvent;
use taskr_core::shared::future::BoxFuture;
use taskr_core::shared::storage::{Revision, SaveError, StorageError};
use taskr_core::tasks::app::TaskListRepository;
use taskr_core::tasks::domain::{TaskList, TaskListSnapshot, TaskSnapshot};
use taskr_core::workspaces::app::{WorkspaceRepository, WorkspaceSummary};
use taskr_core::workspaces::domain::{MemberSnapshot, Workspace, WorkspaceSnapshot};
use uuid::Uuid;

pub use sqlx::postgres::PgPool as Pool;

pub async fn connect(url: &str) -> Result<PgPool, StorageError> {
    let pool = PgPoolOptions::new().max_connections(10).connect(url).await.map_err(db)?;
    sqlx::migrate!("./migrations").run(&pool).await.map_err(|e| StorageError(e.to_string()))?;
    Ok(pool)
}

fn db(e: sqlx::Error) -> StorageError {
    StorageError(format!("database: {e}"))
}

fn save_failed(e: sqlx::Error) -> SaveError {
    match e.as_database_error().and_then(|d| d.code()) {
        Some(code) if code == "23505" => SaveError::Conflict,
        Some(code) if code == "23503" => {
            SaveError::Failed(StorageError("the workspace does not exist".into()))
        }
        _ => SaveError::Failed(db(e)),
    }
}

/// Ids are UUIDs here; anything else cannot name a stored workspace.
fn uuid_of(ws: &WorkspaceId) -> Option<Uuid> {
    Uuid::parse_str(&ws.0).ok()
}

fn rev(value: i64) -> Revision {
    Revision(value as u64)
}

async fn write_outbox(
    tx: &mut Transaction<'_, Postgres>,
    events: &[DomainEvent],
) -> Result<(), SaveError> {
    for e in events {
        let ws = uuid_of(&e.workspace)
            .ok_or_else(|| StorageError(format!("event for unknown workspace {}", e.workspace)))?;
        let payload: Map<String, Value> =
            e.fields.iter().map(|(k, v)| ((*k).to_string(), Value::String(v.clone()))).collect();
        sqlx::query(
            "insert into outbox (workspace_id, name, actor, occurred_at, payload) values ($1, $2, $3, $4, $5)",
        )
        .bind(ws)
        .bind(e.name)
        .bind(&e.actor.0)
        .bind(e.at)
        .bind(Json(Value::Object(payload)))
        .execute(&mut **tx)
        .await
        .map_err(save_failed)?;
    }
    Ok(())
}

/// The aggregate row was written exactly once, or someone else saved first.
/// A first save is a plain insert, so of two racing first saves the second
/// hits the primary key; a later save only updates the revision it loaded.
fn written_once(result: Result<PgQueryResult, sqlx::Error>) -> Result<(), SaveError> {
    match result.map_err(save_failed)?.rows_affected() {
        1 => Ok(()),
        _ => Err(SaveError::Conflict),
    }
}

#[derive(Clone)]
pub struct PgTaskListRepository {
    pool: PgPool,
}

impl PgTaskListRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

type TaskRow = (i32, i64, Option<i32>, Option<String>, Option<i64>, Option<i64>);

impl TaskListRepository for PgTaskListRepository {
    fn load<'a>(
        &'a self,
        workspace: &'a WorkspaceId,
    ) -> BoxFuture<'a, Result<(TaskList, Revision), StorageError>> {
        Box::pin(async move {
            let Some(id) = uuid_of(workspace) else { return Ok((TaskList::new(), Revision::NEW)) };
            let rows: Vec<TaskRow> = sqlx::query_as(
                "select l.next_id, l.revision, t.id, t.title, t.created_at, t.done_at
                 from task_lists l left join tasks t on t.workspace_id = l.workspace_id
                 where l.workspace_id = $1 order by t.id",
            )
            .bind(id)
            .fetch_all(&self.pool)
            .await
            .map_err(db)?;
            let Some(&(next_id, revision, ..)) = rows.first() else {
                return Ok((TaskList::new(), Revision::NEW));
            };
            let tasks = rows
                .into_iter()
                .filter_map(|(_, _, id, title, created_at, done_at)| {
                    Some(TaskSnapshot {
                        id: id? as u32,
                        title: title?,
                        created_at: created_at?,
                        done_at,
                    })
                })
                .collect();
            let list = TaskList::restore(TaskListSnapshot { next_id: next_id as u32, tasks })
                .map_err(|e| StorageError(format!("invalid stored task list: {}", e.0)))?;
            Ok((list, rev(revision)))
        })
    }

    fn save<'a>(
        &'a self,
        workspace: &'a WorkspaceId,
        list: &'a TaskList,
        expected: Revision,
        events: &'a [DomainEvent],
    ) -> BoxFuture<'a, Result<(), SaveError>> {
        Box::pin(async move {
            let id = uuid_of(workspace)
                .ok_or_else(|| StorageError("the workspace does not exist".into()))?;
            let snapshot = list.snapshot();
            let mut tx = self.pool.begin().await.map_err(save_failed)?;
            let next = expected.next().0 as i64;
            let written = if expected == Revision::NEW {
                sqlx::query(
                    "insert into task_lists (workspace_id, next_id, revision) values ($1, $2, $3)",
                )
                .bind(id)
                .bind(snapshot.next_id as i32)
                .bind(next)
                .execute(&mut *tx)
                .await
            } else {
                sqlx::query(
                    "update task_lists set next_id = $2, revision = $3
                     where workspace_id = $1 and revision = $4",
                )
                .bind(id)
                .bind(snapshot.next_id as i32)
                .bind(next)
                .bind(expected.0 as i64)
                .execute(&mut *tx)
                .await
            };
            written_once(written)?;
            sqlx::query("delete from tasks where workspace_id = $1")
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(save_failed)?;
            let ids: Vec<i32> = snapshot.tasks.iter().map(|t| t.id as i32).collect();
            let titles: Vec<&str> = snapshot.tasks.iter().map(|t| t.title.as_str()).collect();
            let created: Vec<i64> = snapshot.tasks.iter().map(|t| t.created_at).collect();
            let done: Vec<Option<i64>> = snapshot.tasks.iter().map(|t| t.done_at).collect();
            sqlx::query(
                "insert into tasks (workspace_id, id, title, created_at, done_at)
                 select $1, * from unnest($2::int[], $3::text[], $4::bigint[], $5::bigint[])",
            )
            .bind(id)
            .bind(&ids)
            .bind(&titles)
            .bind(&created)
            .bind(&done)
            .execute(&mut *tx)
            .await
            .map_err(save_failed)?;
            write_outbox(&mut tx, events).await?;
            tx.commit().await.map_err(save_failed)
        })
    }
}

/// Namespace for personal workspace ids: UUIDv5 of the user id under it.
const PERSONAL_WORKSPACES: Uuid = Uuid::from_u128(0x22d1_1de5_6709_4be8_9605_1c1c_f380_135f);

#[derive(Clone)]
pub struct PgWorkspaceRepository {
    pool: PgPool,
}

impl PgWorkspaceRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn role(s: &str) -> Result<Role, StorageError> {
    Role::parse(s).ok_or_else(|| StorageError(format!("unknown role {s:?} in the database")))
}

impl WorkspaceRepository for PgWorkspaceRepository {
    fn new_id(&self) -> WorkspaceId {
        WorkspaceId(Uuid::now_v7().to_string())
    }

    fn personal_id(&self, user: &UserId) -> WorkspaceId {
        WorkspaceId(Uuid::new_v5(&PERSONAL_WORKSPACES, user.0.as_bytes()).to_string())
    }

    fn list_for<'a>(
        &'a self,
        email: &'a Email,
    ) -> BoxFuture<'a, Result<Vec<WorkspaceSummary>, StorageError>> {
        Box::pin(async move {
            let rows: Vec<(Uuid, String, String)> = sqlx::query_as(
                "select w.id, w.name, m.role from workspace_members m
                 join workspaces w on w.id = m.workspace_id
                 where m.email = $1 order by w.name, w.id",
            )
            .bind(email.as_str())
            .fetch_all(&self.pool)
            .await
            .map_err(db)?;
            rows.into_iter()
                .map(|(id, name, r)| {
                    Ok(WorkspaceSummary { id: WorkspaceId(id.to_string()), name, role: role(&r)? })
                })
                .collect()
        })
    }

    fn load<'a>(
        &'a self,
        id: &'a WorkspaceId,
    ) -> BoxFuture<'a, Result<Option<(Workspace, Revision)>, StorageError>> {
        Box::pin(async move {
            let Some(uuid) = uuid_of(id) else { return Ok(None) };
            let rows: Vec<(String, i64, String, String)> = sqlx::query_as(
                "select w.name, w.revision, m.email, m.role from workspaces w
                 join workspace_members m on m.workspace_id = w.id
                 where w.id = $1 order by m.position",
            )
            .bind(uuid)
            .fetch_all(&self.pool)
            .await
            .map_err(db)?;
            let Some((name, revision, ..)) = rows.first().cloned() else { return Ok(None) };
            let members = rows
                .into_iter()
                .map(|(_, _, email, r)| Ok(MemberSnapshot { email, role: role(&r)? }))
                .collect::<Result<Vec<_>, StorageError>>()?;
            let ws = Workspace::restore(WorkspaceSnapshot { id: id.clone(), name, members })
                .map_err(StorageError)?;
            Ok(Some((ws, rev(revision))))
        })
    }

    fn save<'a>(
        &'a self,
        workspace: &'a Workspace,
        expected: Revision,
        events: &'a [DomainEvent],
    ) -> BoxFuture<'a, Result<(), SaveError>> {
        Box::pin(async move {
            let id = uuid_of(workspace.id())
                .ok_or_else(|| StorageError(format!("not a uuid: {}", workspace.id())))?;
            let mut tx = self.pool.begin().await.map_err(save_failed)?;
            let next = expected.next().0 as i64;
            let written = if expected == Revision::NEW {
                sqlx::query("insert into workspaces (id, name, revision) values ($1, $2, $3)")
                    .bind(id)
                    .bind(workspace.name())
                    .bind(next)
                    .execute(&mut *tx)
                    .await
            } else {
                sqlx::query(
                    "update workspaces set name = $2, revision = $3 where id = $1 and revision = $4",
                )
                .bind(id)
                .bind(workspace.name())
                .bind(next)
                .bind(expected.0 as i64)
                .execute(&mut *tx)
                .await
            };
            written_once(written)?;
            sqlx::query("delete from workspace_members where workspace_id = $1")
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(save_failed)?;
            let members = workspace.members();
            let emails: Vec<&str> = members.iter().map(|m| m.email.as_str()).collect();
            let roles: Vec<&str> = members.iter().map(|m| m.role.as_str()).collect();
            let positions: Vec<i32> = (0..members.len() as i32).collect();
            sqlx::query(
                "insert into workspace_members (workspace_id, email, role, position)
                 select $1, * from unnest($2::text[], $3::text[], $4::int[])",
            )
            .bind(id)
            .bind(&emails)
            .bind(&roles)
            .bind(&positions)
            .execute(&mut *tx)
            .await
            .map_err(save_failed)?;
            write_outbox(&mut tx, events).await?;
            tx.commit().await.map_err(save_failed)
        })
    }
}

/// An unpublished outbox row, for the relay (and for tests).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutboxRow {
    pub id: i64,
    pub workspace: WorkspaceId,
    pub name: String,
    pub actor: String,
    pub occurred_at: i64,
    pub payload: BTreeMap<String, String>,
}

type OutboxTuple = (i64, String, String, i64, Json<BTreeMap<String, String>>);

pub async fn pending_events(
    pool: &PgPool,
    workspace: &WorkspaceId,
) -> Result<Vec<OutboxRow>, StorageError> {
    let Some(id) = uuid_of(workspace) else { return Ok(vec![]) };
    let rows: Vec<OutboxTuple> = sqlx::query_as(
        "select id, name, actor, occurred_at, payload from outbox
         where workspace_id = $1 and published_at is null order by id",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(db)?;
    Ok(rows
        .into_iter()
        .map(|(id, name, actor, occurred_at, Json(payload))| OutboxRow {
            id,
            workspace: workspace.clone(),
            name,
            actor,
            occurred_at,
            payload,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use taskr_core::testing::{
        block_on, task_list_repository_contract, workspace_repository_contract,
    };

    /// Postgres tests run only when a database is given, for example:
    /// `TASKR_TEST_DATABASE_URL=postgres://taskr:taskr@localhost:54329/taskr`.
    fn database() -> Option<(tokio::runtime::Runtime, PgPool)> {
        let Ok(url) = std::env::var("TASKR_TEST_DATABASE_URL") else {
            eprintln!("skipped: TASKR_TEST_DATABASE_URL is not set");
            return None;
        };
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let pool = rt.block_on(connect(&url)).expect("test database reachable");
        Some((rt, pool))
    }

    fn create_workspace(repo: &PgWorkspaceRepository) -> WorkspaceId {
        let owner = Email::parse("owner@example.com").unwrap();
        let ws = Workspace::create(repo.new_id(), "Test", owner).unwrap();
        block_on(repo.save(&ws, Revision::NEW, &[])).unwrap();
        ws.id().clone()
    }

    #[test]
    fn task_lists_honour_the_repository_contract() {
        let Some((rt, pool)) = database() else { return };
        let _ctx = rt.enter();
        let workspaces = PgWorkspaceRepository::new(pool.clone());
        task_list_repository_contract(
            || PgTaskListRepository::new(pool.clone()),
            |r| r.clone(),
            |_| create_workspace(&workspaces),
        );
    }

    #[test]
    fn workspaces_honour_the_repository_contract() {
        let Some((rt, pool)) = database() else { return };
        let _ctx = rt.enter();
        workspace_repository_contract(PgWorkspaceRepository::new(pool));
    }

    #[test]
    fn events_are_written_with_the_change_and_not_without_it() {
        let Some((rt, pool)) = database() else { return };
        let _ctx = rt.enter();
        let ws = create_workspace(&PgWorkspaceRepository::new(pool.clone()));
        let repo = PgTaskListRepository::new(pool.clone());
        let event = DomainEvent {
            name: "task.added",
            workspace: ws.clone(),
            actor: taskr_core::shared::access::UserId("u1".into()),
            at: 42,
            fields: vec![("title", "Buy milk".into())],
        };
        block_on(repo.save(&ws, &TaskList::new(), Revision::NEW, std::slice::from_ref(&event)))
            .unwrap();
        assert_eq!(
            block_on(repo.save(&ws, &TaskList::new(), Revision::NEW, &[event])),
            Err(SaveError::Conflict)
        );
        let rows = rt.block_on(pending_events(&pool, &ws)).unwrap();
        assert_eq!(rows.len(), 1, "the conflicting save wrote nothing");
        assert_eq!(rows[0].name, "task.added");
        assert_eq!(rows[0].actor, "u1");
        assert_eq!(rows[0].payload.get("title").map(String::as_str), Some("Buy milk"));
    }

    /// Runs `insert` in a transaction left open, starts `save` so it queues
    /// behind it, then commits: the save must see the other first save.
    fn first_save_behind(
        rt: &tokio::runtime::Runtime,
        pool: &PgPool,
        insert: &'static str,
        id: Uuid,
        save: impl Future<Output = Result<(), SaveError>> + Send + 'static,
    ) -> Result<(), SaveError> {
        rt.block_on(async {
            let mut other = pool.begin().await.unwrap();
            sqlx::query(insert).bind(id).execute(&mut *other).await.unwrap();
            let save = tokio::spawn(save);
            let waiting = "select count(*) from pg_stat_activity
                 where wait_event_type = 'Lock' and query like 'insert into%' and query like '%' || $1 || '%'";
            let mut blocked = false;
            for _ in 0..500 {
                let n: i64 = sqlx::query_scalar(waiting)
                    .bind(insert.split_whitespace().nth(2).unwrap())
                    .fetch_one(pool)
                    .await
                    .unwrap();
                if n > 0 || save.is_finished() {
                    blocked = n > 0;
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            other.commit().await.unwrap();
            let result = save.await.unwrap();
            assert!(blocked || result.is_err(), "the save neither waited nor failed: {result:?}");
            result
        })
    }

    #[test]
    fn concurrent_first_task_saves_cannot_both_win() {
        let Some((rt, pool)) = database() else { return };
        let _ctx = rt.enter();
        let ws = create_workspace(&PgWorkspaceRepository::new(pool.clone()));
        let repo = PgTaskListRepository::new(pool.clone());
        let (save_ws, save_repo) = (ws.clone(), repo.clone());
        let result = first_save_behind(
            &rt,
            &pool,
            "insert into task_lists (workspace_id, next_id, revision) values ($1, 1, 1)",
            uuid_of(&ws).unwrap(),
            async move { save_repo.save(&save_ws, &TaskList::new(), Revision::NEW, &[]).await },
        );
        assert_eq!(result, Err(SaveError::Conflict));
        assert_eq!(block_on(repo.load(&ws)).unwrap().1, Revision(1));
    }

    #[test]
    fn concurrent_workspace_creations_cannot_both_win() {
        let Some((rt, pool)) = database() else { return };
        let _ctx = rt.enter();
        let repo = PgWorkspaceRepository::new(pool.clone());
        let ws = Workspace::create(repo.new_id(), "Mine", Email::parse("me@example.com").unwrap())
            .unwrap();
        let id = uuid_of(ws.id()).unwrap();
        let save_repo = repo.clone();
        let result = first_save_behind(
            &rt,
            &pool,
            "insert into workspaces (id, name, revision) values ($1, 'Theirs', 1)",
            id,
            async move { save_repo.save(&ws, Revision::NEW, &[]).await },
        );
        assert_eq!(result, Err(SaveError::Conflict));
    }

    #[test]
    fn non_uuid_ids_read_as_missing() {
        let Some((rt, pool)) = database() else { return };
        let _ctx = rt.enter();
        let ws = WorkspaceId("not-a-uuid".into());
        assert_eq!(block_on(PgWorkspaceRepository::new(pool.clone()).load(&ws)).unwrap(), None);
        assert_eq!(
            block_on(PgTaskListRepository::new(pool).load(&ws)).unwrap(),
            (TaskList::new(), Revision::NEW)
        );
    }
}
