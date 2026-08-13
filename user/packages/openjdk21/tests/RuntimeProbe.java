import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.atomic.AtomicInteger;

public class RuntimeProbe {
    public static void main(String[] args) throws Exception {
        AtomicInteger completed = new AtomicInteger();
        Thread[] threads = new Thread[4];
        for (int i = 0; i < threads.length; i++) {
            threads[i] = new Thread(() -> {
                long sum = 0;
                for (int n = 0; n < 200000; n++) {
                    sum += (n * 31L) ^ (n >>> 3);
                }
                if (sum != 620002257824L) {
                    throw new AssertionError("unexpected sum: " + sum);
                }
                completed.incrementAndGet();
            }, "wateros-jvm-" + i);
            threads[i].start();
        }
        for (Thread thread : threads) {
            thread.join();
        }
        List<byte[]> allocations = new ArrayList<>();
        for (int i = 0; i < 128; i++) {
            allocations.add(new byte[256 * 1024]);
        }
        allocations.clear();
        System.gc();
        if (completed.get() != threads.length) {
            throw new AssertionError("threads did not complete");
        }
        System.out.println("WATEROS_JVM_RUNTIME_OK threads=" + completed.get());
    }
}
