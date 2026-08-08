// Smoke test for the Java binding: record → resume → replay parity, durable
// effects, typed faults, channels, and hash-chain verification - the same
// scenario the Rust, Python, C ABI, and C++ tests run.

import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;
import java.util.concurrent.atomic.AtomicInteger;

import pragmatic.Pragmatic;

public final class Smoke {
    static void check(boolean cond, String what) {
        if (!cond) throw new AssertionError(what);
    }

    public static void main(String[] args) throws Exception {
        Path dir = Files.createTempDirectory("prag-java-smoke-");

        AtomicInteger oracleCalls = new AtomicInteger();
        AtomicInteger effectPerforms = new AtomicInteger();

        try (var rt = new Pragmatic.Runtime(
                dir.toString(),
                prompt -> "completion#" + oracleCalls.getAndIncrement() + "(" + prompt + ")",
                "java-smoke-key".getBytes())) {

            Pragmatic.Agent agent = ctx -> {
                String plan = ctx.oracle("plan the task");
                List<String> findings = new ArrayList<>();
                for (int step = 0; step < 3; step++) {
                    findings.add(ctx.oracle("probe " + step + ": " + plan));
                }
                String published = ctx.effect(
                        "publish", findings.size() + " findings",
                        arg -> {
                            effectPerforms.incrementAndGet();
                            return "s3://reports/" + arg;
                        });
                return "report(" + published + ")";
            };

            // Record.
            List<Pragmatic.TraceLabel> recordedTrace;
            try (var report = rt.run("java-research-1", agent)) {
                check(report.output().equals("report(s3://reports/3 findings)"),
                        "output: " + report.output());
                check(oracleCalls.get() == 4, "oracle calls: " + oracleCalls);
                check(effectPerforms.get() == 1, "effect performs: " + effectPerforms);
                check(report.freshSteps() == 4, "fresh: " + report.freshSteps());
                check(report.replayedSteps() == 0, "replayed: " + report.replayedSteps());
                check(report.chainHead().length() == 64, "chain head");
                recordedTrace = report.trace();
            }

            // Resume: everything comes from the journal - no model calls, no
            // re-performed effect.
            try (var resumed = rt.resume("java-research-1", agent)) {
                check(oracleCalls.get() == 4, "resume re-sampled");
                check(effectPerforms.get() == 1, "resume re-performed effect");
                check(resumed.freshSteps() == 0, "resume fresh: " + resumed.freshSteps());
            }

            // Replay: bit-for-bit trace parity (T1), model never consulted.
            try (var audit = rt.replay("java-research-1", agent)) {
                check(oracleCalls.get() == 4, "replay hit the model");
                check(audit.trace().equals(recordedTrace), "trace parity");
            }

            // The chain verifies end to end.
            check(rt.verify("java-research-1"), "verify");

            // Channels.
            rt.send("java-approval-1", "approvals", "approved");
            try (var approval = rt.run("java-approval-1",
                    ctx -> ctx.recv("approvals"))) {
                check(approval.output().equals("approved"), "recv");
            }

            // Typed faults surface with the right code.
            boolean threw = false;
            try (var ignored = rt.run("java-fault-1", ctx -> {
                ctx.contract(false, "must not continue");
                return "unreachable";
            })) {
                // not reached
            } catch (Pragmatic.FaultException e) {
                threw = true;
                check(e.code() == Pragmatic.ERR_CONTRACT, "fault code: " + e.code());
                check(e.getMessage().contains("must not continue"), "fault msg: " + e.getMessage());
            }
            check(threw, "contract fault");

            // A Java exception from an agent is caught at the boundary.
            threw = false;
            try (var ignored = rt.run("java-throw-1", ctx -> {
                throw new IllegalStateException("agent exploded");
            })) {
                // not reached
            } catch (Pragmatic.FaultException e) {
                threw = true;
                check(e.getMessage().contains("agent exploded"), "exception msg: " + e.getMessage());
            }
            check(threw, "exception boundary");
        }

        System.out.println("java smoke: OK (runtime v" + Pragmatic.version() + ")");

        try (var walk = Files.walk(dir)) {
            walk.sorted(Comparator.reverseOrder()).forEach(p -> p.toFile().delete());
        }
    }
}
