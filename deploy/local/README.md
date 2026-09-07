# Local development

Run one `aequora-server`, PostgreSQL, and a client using SQLite or Stoolap. Filesystem snapshot
storage and test authentication are permitted only with the development profile. A container
runtime is optional. Device tests may bind to a trusted LAN, but retain authentication and normal
protocol semantics. Never reuse this profile in staging or production.

