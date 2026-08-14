Minecraft Java Edition server 1.21.11 for WaterOS

Official download page:
  https://www.minecraft.net/en-us/download/server
Official release page:
  https://www.minecraft.net/en-us/article/minecraft-java-edition-1-21-11
Pinned server object SHA-1:
  64bb6d763bed0a9f1d632ec347938594144943ed

Downloading and running the server are governed by the Minecraft EULA and
Privacy Policy. WaterOS does not pre-accept the EULA. Review it first, then run:

  minecraft-server --accept-eula
  minecraft-server

The default data directory is /var/lib/minecraft. Set MINECRAFT_DATA_DIR and
MINECRAFT_JAVA_ARGS to override the data directory and JVM memory/options.
On RISC-V, the WaterOS default currently caps tiered compilation at C1 because
complex C2-generated Minecraft code still crashes. C1 works after WaterOS made
synchronous fault siginfo Linux-compatible. LoongArch keeps normal tiered
compilation. Set MINECRAFT_JAVA_ARGS only when deliberately testing a different
VM configuration.

The destructive acceptance test uses an isolated directory under /tmp:

  wos-minecraft-preflight
  wos-minecraft-vm-info
  wos-minecraft-smoke

The preflight does not accept the EULA or create a world. It verifies the Java
21 bundler/class-loading path and requires the generated eula.txt to remain
false. The smoke test requires prior explicit acceptance and starts a server.
The VM-info command verifies that no environment option enables HotSpot's
SelfDestructTimer and that its initial value is zero.

The developer-only wos-minecraft-jit-diagnostic command runs normal tiered JIT
and persists its log/crash report in /var/lib/minecraft/jit-diagnostic. Run it
only on a disposable writable image while reducing a JVM compatibility issue.
