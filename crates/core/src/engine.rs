//! The change engine: observe → plan → apply → verify.
//!
//! Every Cloudflare mutation goes through this pipeline. The planner is pure; the
//! executor applies a reviewed plan with a staleness guard, logs every step and
//! compensates completed steps on failure.
