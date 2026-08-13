import java.util.concurrent.atomic.AtomicReference;

public final class ExceptionProbe {
    private static boolean finallyExecuted;

    private static void recurse(int depth) {
        if (depth == 0) {
            throw new IllegalStateException("wateros-expected");
        }
        recurse(depth - 1);
    }

    public static void main(String[] args) throws Exception {
        boolean caught = false;
        try {
            recurse(32);
        } catch (IllegalStateException exception) {
            caught = "wateros-expected".equals(exception.getMessage())
                    && exception.getStackTrace().length >= 16;
        } finally {
            finallyExecuted = true;
        }
        if (!caught || !finallyExecuted) {
            throw new AssertionError("exception unwind/catch/finally failed");
        }

        AtomicReference<Throwable> uncaught = new AtomicReference<>();
        Thread thread = new Thread(
                () -> { throw new RuntimeException("wateros-thread-exception"); },
                "wateros-exception-thread");
        thread.setUncaughtExceptionHandler((ignored, exception) -> uncaught.set(exception));
        thread.start();
        thread.join();

        Throwable failure = uncaught.get();
        if (!(failure instanceof RuntimeException)
                || !"wateros-thread-exception".equals(failure.getMessage())) {
            throw new AssertionError("thread exception delivery failed", failure);
        }
        System.out.println(
                "WATEROS_JVM_EXCEPTION_OK stack-unwind=true finally=true thread=true");
    }
}
