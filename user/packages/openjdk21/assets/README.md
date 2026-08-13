# Java CA truststore

`cacerts` is a JKS truststore generated from every certificate in Alpine
v3.22's RISC-V `ca-certificates-bundle-20260611-r0.apk`.

- Source APK SHA-256: `537dcb625ede1cb81e751dd92552b2715a35fdd72cdb43a965a055f14900d529`
- JKS SHA-256: `d8688143c6107456a13d959ae23c9f375ee2b743c8bb5f59be77d9b5ac956173`
- Store type/password: `JKS` / `changeit` (the standard public truststore password)
- Trusted root entries: 119

The JKS is installed at `/etc/ssl/certs/java/cacerts`, which is the target of
OpenJDK's default `lib/security/cacerts` symlink. The APK itself supplies the
matching system PEM bundle at `/etc/ssl/certs/ca-certificates.crt`.
