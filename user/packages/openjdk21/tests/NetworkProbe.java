import java.io.InputStream;
import java.net.InetAddress;
import java.net.InetSocketAddress;
import java.net.URI;
import java.net.Socket;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.KeyStore;
import javax.net.ssl.HttpsURLConnection;
import javax.net.ssl.SSLSession;
import javax.net.ssl.SSLSocket;
import javax.net.ssl.SSLSocketFactory;

public class NetworkProbe {
    private static final int TIMEOUT_MS = 15_000;

    public static void main(String[] args) throws Exception {
        String host = args.length == 0 ? "example.com" : args[0];
        Path trustStore = Path.of("/etc/ssl/certs/java/cacerts");
        if (!Files.isRegularFile(trustStore)) {
            throw new AssertionError("missing Java truststore: " + trustStore);
        }

        KeyStore keys = KeyStore.getInstance("JKS");
        try (InputStream input = Files.newInputStream(trustStore)) {
            keys.load(input, "changeit".toCharArray());
        }
        if (keys.size() < 100) {
            throw new AssertionError("too few trusted roots: " + keys.size());
        }
        System.out.println("WATEROS_JVM_TRUST_OK entries=" + keys.size());

        InetAddress[] addresses = InetAddress.getAllByName(host);
        if (addresses.length == 0) {
            throw new AssertionError("DNS returned no addresses for " + host);
        }
        System.out.println("WATEROS_JVM_DNS_OK host=" + host
                + " address=" + addresses[0].getHostAddress());

        try (Socket socket = new Socket()) {
            socket.connect(new InetSocketAddress(host, 443), TIMEOUT_MS);
            System.out.println("WATEROS_JVM_TCP_OK remote=" + socket.getRemoteSocketAddress());
        }

        SSLSocketFactory factory = (SSLSocketFactory) SSLSocketFactory.getDefault();
        try (SSLSocket socket = (SSLSocket) factory.createSocket(host, 443)) {
            socket.setSoTimeout(TIMEOUT_MS);
            socket.startHandshake();
            SSLSession session = socket.getSession();
            if (!session.isValid()) {
                throw new AssertionError("TLS session is invalid");
            }
            System.out.println("WATEROS_JVM_TLS_OK protocol=" + session.getProtocol()
                    + " cipher=" + session.getCipherSuite());
        }

        HttpsURLConnection connection =
                (HttpsURLConnection) URI.create("https://" + host + "/").toURL().openConnection();
        connection.setConnectTimeout(TIMEOUT_MS);
        connection.setReadTimeout(TIMEOUT_MS);
        connection.setRequestProperty("User-Agent", "WaterOS-JVM-Probe/1");
        int status = connection.getResponseCode();
        if (status < 200 || status >= 400) {
            throw new AssertionError("unexpected HTTPS status: " + status);
        }
        try (InputStream body = connection.getInputStream()) {
            if (body.read() < 0) {
                throw new AssertionError("empty HTTPS response");
            }
        } finally {
            connection.disconnect();
        }
        System.out.println("WATEROS_JVM_HTTPS_OK status=" + status);
    }
}
