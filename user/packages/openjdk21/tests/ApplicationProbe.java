import java.io.InputStream;
import java.net.InetAddress;
import java.net.InetSocketAddress;
import java.nio.ByteBuffer;
import java.nio.MappedByteBuffer;
import java.nio.channels.FileChannel;
import java.nio.channels.FileLock;
import java.nio.channels.SelectionKey;
import java.nio.channels.Selector;
import java.nio.channels.ServerSocketChannel;
import java.nio.channels.SocketChannel;
import java.nio.charset.StandardCharsets;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.nio.file.StandardOpenOption;
import java.util.Arrays;
import java.util.Iterator;
import java.util.concurrent.atomic.AtomicReference;

public class ApplicationProbe {
    private static final byte[] FILE_BYTES = new byte[1024 * 1024];

    public static void main(String[] args) throws Exception {
        for (int i = 0; i < FILE_BYTES.length; i++) {
            FILE_BYTES[i] = (byte) (i * 31 + 7);
        }
        probeJarAndResources();
        probeFileSystem();
        probeProcessBuilder();
        probeNioSelector();
        System.out.println("WATEROS_JVM_APPLICATION_OK");
    }

    private static void probeJarAndResources() throws Exception {
        String location = ApplicationProbe.class.getProtectionDomain()
                                                .getCodeSource()
                                                .getLocation()
                                                .toString();
        if (!location.endsWith("ApplicationProbe.jar")) {
            throw new AssertionError("probe was not loaded from its JAR: " + location);
        }
        try (InputStream input = ApplicationProbe.class.getResourceAsStream("/probe-resource.txt")) {
            if (input == null) {
                throw new AssertionError("JAR resource is missing");
            }
            String text = new String(input.readAllBytes(), StandardCharsets.UTF_8).trim();
            if (!text.equals("WaterOS-JAR-resource-中文")) {
                throw new AssertionError("unexpected JAR resource: " + text);
            }
        }
        Class<?> loaded = Class.forName("ApplicationProbe");
        if (loaded != ApplicationProbe.class) {
            throw new AssertionError("reflection returned another class");
        }
        System.out.println("WATEROS_JVM_JAR_OK location=" + location);
    }

    private static void probeFileSystem() throws Exception {
        Path directory = Files.createTempDirectory("wateros-jvm-");
        Path data = directory.resolve("mapped-data.bin");
        Path renamed = directory.resolve("renamed-data.bin");
        boolean atomicMove = true;
        try {
            try (FileChannel channel = FileChannel.open(data,
                                                        StandardOpenOption.CREATE_NEW,
                                                        StandardOpenOption.READ,
                                                        StandardOpenOption.WRITE)) {
                ByteBuffer source = ByteBuffer.wrap(FILE_BYTES);
                while (source.hasRemaining()) {
                    channel.write(source);
                }
                channel.force(true);
                try (FileLock lock = channel.lock()) {
                    if (!lock.isValid()) {
                        throw new AssertionError("file lock is invalid");
                    }
                    MappedByteBuffer mapped = channel.map(FileChannel.MapMode.READ_WRITE,
                                                          0,
                                                          FILE_BYTES.length);
                    mapped.put(0, (byte) 0x5a);
                    mapped.put(FILE_BYTES.length - 1, (byte) 0x6b);
                    mapped.force();
                }
            }
            try {
                Files.move(data, renamed, StandardCopyOption.ATOMIC_MOVE);
            } catch (AtomicMoveNotSupportedException unsupported) {
                atomicMove = false;
                Files.move(data, renamed);
            }
            byte[] actual = Files.readAllBytes(renamed);
            if (actual.length != FILE_BYTES.length
                    || actual[0] != (byte) 0x5a
                    || actual[actual.length - 1] != (byte) 0x6b
                    || !Arrays.equals(Arrays.copyOfRange(actual, 1, actual.length - 1),
                                      Arrays.copyOfRange(FILE_BYTES, 1, FILE_BYTES.length - 1))) {
                throw new AssertionError("mapped file content mismatch");
            }
            System.out.println("WATEROS_JVM_NIO_FILE_OK bytes=" + actual.length
                    + " atomicMove=" + atomicMove);
        } finally {
            Files.deleteIfExists(renamed);
            Files.deleteIfExists(data);
            Files.deleteIfExists(directory);
        }
    }

    private static void probeProcessBuilder() throws Exception {
        ProcessBuilder builder = new ProcessBuilder(
                "/bin/sh", "-c",
                "printf '%s' \"$WATEROS_JAVA_CHILD\"; printf child-error >&2; exit 7");
        builder.environment().put("WATEROS_JAVA_CHILD", "child-output");
        Process process = builder.start();
        byte[] stdout;
        byte[] stderr;
        try (InputStream output = process.getInputStream();
             InputStream error = process.getErrorStream()) {
            stdout = output.readAllBytes();
            stderr = error.readAllBytes();
        }
        int status = process.waitFor();
        String out = new String(stdout, StandardCharsets.UTF_8);
        String err = new String(stderr, StandardCharsets.UTF_8);
        if (status != 7 || !out.equals("child-output") || !err.equals("child-error")) {
            throw new AssertionError("ProcessBuilder mismatch: status=" + status
                    + " stdout=" + out + " stderr=" + err);
        }
        System.out.println("WATEROS_JVM_PROCESS_OK exit=" + status);
    }

    private static void probeNioSelector() throws Exception {
        AtomicReference<Throwable> clientFailure = new AtomicReference<>();
        try (Selector selector = Selector.open();
             ServerSocketChannel server = ServerSocketChannel.open()) {
            server.configureBlocking(false);
            server.bind(new InetSocketAddress(InetAddress.getLoopbackAddress(), 0));
            server.register(selector, SelectionKey.OP_ACCEPT);
            InetSocketAddress address = (InetSocketAddress) server.getLocalAddress();

            Thread client = new Thread(() -> {
                try (SocketChannel socket = SocketChannel.open(address)) {
                    writeFully(socket, ByteBuffer.wrap("ping".getBytes(StandardCharsets.US_ASCII)));
                    ByteBuffer reply = ByteBuffer.allocate(4);
                    readFully(socket, reply);
                    if (!new String(reply.array(), StandardCharsets.US_ASCII).equals("pong")) {
                        throw new AssertionError("unexpected loopback reply");
                    }
                } catch (Throwable failure) {
                    clientFailure.set(failure);
                }
            }, "wateros-nio-client");
            client.start();

            SocketChannel accepted = null;
            long deadline = System.nanoTime() + 10_000_000_000L;
            while (accepted == null && System.nanoTime() < deadline) {
                selector.select(1000);
                Iterator<SelectionKey> keys = selector.selectedKeys().iterator();
                while (keys.hasNext()) {
                    SelectionKey key = keys.next();
                    keys.remove();
                    if (key.isAcceptable()) {
                        accepted = server.accept();
                    }
                }
            }
            if (accepted == null) {
                throw new AssertionError("selector did not report an accepted connection");
            }
            try (SocketChannel socket = accepted) {
                socket.configureBlocking(true);
                ByteBuffer request = ByteBuffer.allocateDirect(4);
                readFully(socket, request);
                request.flip();
                byte[] requestBytes = new byte[4];
                request.get(requestBytes);
                if (!new String(requestBytes, StandardCharsets.US_ASCII).equals("ping")) {
                    throw new AssertionError("unexpected loopback request");
                }
                writeFully(socket, ByteBuffer.wrap("pong".getBytes(StandardCharsets.US_ASCII)));
            }
            client.join(10_000);
            if (client.isAlive()) {
                throw new AssertionError("loopback client did not exit");
            }
            if (clientFailure.get() != null) {
                throw new AssertionError("loopback client failed", clientFailure.get());
            }
            System.out.println("WATEROS_JVM_NIO_SELECTOR_OK port=" + address.getPort());
        }
    }

    private static void writeFully(SocketChannel channel, ByteBuffer buffer) throws Exception {
        while (buffer.hasRemaining()) {
            channel.write(buffer);
        }
    }

    private static void readFully(SocketChannel channel, ByteBuffer buffer) throws Exception {
        while (buffer.hasRemaining()) {
            if (channel.read(buffer) < 0) {
                throw new AssertionError("unexpected end of stream");
            }
        }
    }
}
