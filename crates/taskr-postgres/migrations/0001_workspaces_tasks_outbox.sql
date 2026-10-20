create table workspaces (
    id uuid primary key,
    name text not null,
    revision bigint not null,
    created_at timestamptz not null default now()
);

create table workspace_members (
    workspace_id uuid not null references workspaces (id) on delete cascade,
    email text not null,
    role text not null check (role in ('owner', 'editor', 'viewer')),
    position int not null,
    primary key (workspace_id, email)
);

create index workspace_members_by_email on workspace_members (email);

create table task_lists (
    workspace_id uuid primary key references workspaces (id) on delete cascade,
    next_id int not null,
    revision bigint not null
);

create table tasks (
    workspace_id uuid not null references task_lists (workspace_id) on delete cascade,
    id int not null,
    title text not null,
    created_at bigint not null,
    done_at bigint,
    primary key (workspace_id, id)
);

-- Written in the same transaction as the change it describes. A relay reads
-- unpublished rows in id order and marks them published (analytics, realtime).
create table outbox (
    id bigserial primary key,
    workspace_id uuid not null,
    name text not null,
    actor text not null,
    occurred_at bigint not null,
    payload jsonb not null,
    published_at timestamptz
);

create index outbox_unpublished on outbox (id) where published_at is null;
