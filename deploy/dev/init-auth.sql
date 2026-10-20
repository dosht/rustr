-- What Supabase's own images set up before GoTrue runs its migrations.
create user supabase_auth_admin noinherit createrole login password 'auth';
create schema if not exists auth authorization supabase_auth_admin;
grant create on database taskr to supabase_auth_admin;
alter user supabase_auth_admin set search_path = 'auth';
