//! `as_of` queries — reconstruct the memory palace as it was at a given moment.
//!
//! Filters every retrieval by `valid_from <= as_of <= valid_to`, with sensible
//! defaults when `valid_to` is `NULL` (still valid).

// TODO(V1): plumb `as_of` through KG, diary, and hybrid search APIs.
