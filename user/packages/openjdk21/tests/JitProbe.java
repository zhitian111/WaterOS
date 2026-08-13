public final class JitProbe {
    public static long hot(long seed) {
        long value = seed;
        for (int i = 0; i < 256; i++) {
            value = (value * 2862933555777941757L + 3037000493L)
                    ^ (value >>> 17);
        }
        return value;
    }

    private static long reference(long seed) {
        long value = seed;
        for (int i = 0; i < 256; i++) {
            value = (value * 2862933555777941757L + 3037000493L)
                    ^ (value >>> 17);
        }
        return value;
    }

    public static void main(String[] args) {
        long checksum = 0;
        for (int iteration = 0; iteration < 20000; iteration++) {
            long compiled = hot(iteration);
            long interpreted = reference(iteration);
            if (compiled != interpreted) {
                throw new AssertionError(
                        "JIT mismatch at " + iteration
                        + ": compiled=" + compiled
                        + " interpreted=" + interpreted);
            }
            checksum ^= compiled;
        }
        System.out.println(
                "WATEROS_JVM_JIT_OK iterations=20000 checksum=" + checksum);
    }
}
