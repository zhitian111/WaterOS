# Upstream provenance

The compact grid layout in this crate is derived from:

- `tabled` 0.20.0, especially `src/tables/compact.rs`
- `papergrid` 0.17.0, especially its compact grid, configuration, dimension,
  records, and string-width modules
- upstream repository: <https://github.com/zhiburt/tabled>
- license: MIT

The audited crates.io archives have these SHA-256 checksums:

```text
tabled-0.20.0.crate:
e39a2ee1fbcd360805a771e1b300f78cc88fec7b8d3e2f71cd37bbf23e725c7d

papergrid-0.17.0.crate:
6978128c8b51d8f4080631ceb2302ab51e32cc6e8615f735ee2f83fd269ae3f1
```

WaterOS changes:

- reduced the implementation to single-line compact tables;
- removed `std`, allocation, terminal detection, ANSI colors, spans and
  mutable setting machinery;
- added a fixed-width streaming API accepting borrowed `Display` and `Debug`
  values;
- retained a zero-allocation automatic-width API for rectangular string
  records;
- made all output flow through `core::fmt::Write`.

