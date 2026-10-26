-- Enrollment is repeatable and always made by the signed-in account itself, so its verified
-- contact facts may be refreshed. The creation time is never updatable: a refresh cannot move
-- the free credits' window or grant them again.
GRANT UPDATE (email, full_name) ON taskr_billing.customers TO billing_runtime;
