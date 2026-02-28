//! Server switching behavior.
//!
//! Selecting a server updates connection state, dashboard refreshes, and view-local selections in
//! one place so callers do not have to coordinate those steps manually.

mod switch;
