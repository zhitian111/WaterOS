# Third-party components used by real-hardware ports

The machine-readable inventory is [`third-party-components.json`](third-party-components.json).
It covers libraries currently linked by the kernel or vendored for the root
filesystem. `UNKNOWN` is intentional: it blocks redistribution review and is
not a license assertion. Run the inventory test before importing another
upstream driver or changing a version.

The inventory does not replace the license notices shipped by upstream
projects. Registry dependencies remain pinned by `os/Cargo.lock`; vendored
components must retain their upstream notice files.
