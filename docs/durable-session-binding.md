# Trusted Session transcript staging boundary

**Status:** design specification only. This document describes a proposed core-owned staging boundary; it does not implement or authorize a live cutover, native transcript mutation, deployment, or public import API.

## Authority and goal

A durable transcript may be staged only from the exact native file recorded in the authoritative CPD `sessions.file` field for the requested existing Session. The caller's Session ID, a filename, a pi header ID, and a native `delivered_to` marker are not proof of ownership. Snapshot validation and source ownership are separate checks: valid JSONL and a matching digest do not establish which CPD Session owns the bytes.
