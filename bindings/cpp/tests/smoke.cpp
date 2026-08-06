// Smoke test for the C++ binding: record → resume → replay parity, durable
// effects, typed faults, channels, and hash-chain verification — the same
// scenario the Rust, Python, and C ABI tests run.

#include <unistd.h>

#include <cassert>
#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <string>
#include <vector>

#include "../pragmatic.hpp"

int main() {
    namespace fs = std::filesystem;
    const fs::path dir =
        fs::temp_directory_path() / ("prag-cpp-smoke-" + std::to_string(::getpid()));
    fs::remove_all(dir);

    int oracle_calls = 0;
    pragmatic::Runtime rt(
        dir.string(),
        [&oracle_calls](const std::string& prompt) {
            return "completion#" + std::to_string(oracle_calls++) + "(" + prompt + ")";
        },
        "cpp-smoke-key");

    int effect_performs = 0;
    auto agent = [&effect_performs](pragmatic::Ctx& ctx) {
        auto plan = ctx.oracle("plan the task");
        std::vector<std::string> findings;
        for (int step = 0; step < 3; ++step) {
            findings.push_back(ctx.oracle("probe " + std::to_string(step) + ": " + plan));
        }
        auto published = ctx.effect(
            "publish", std::to_string(findings.size()) + " findings",
            [&effect_performs](const std::string& arg) {
                ++effect_performs;
                return "s3://reports/" + arg;
            });
        return "report(" + published + ")";
    };

    // Record.
    auto report = rt.run("cpp-research-1", agent);
    assert(report.output() == "report(s3://reports/3 findings)");
    assert(oracle_calls == 4);
    assert(effect_performs == 1);
    assert(report.fresh_steps() == 4); // oracle draws (effects tracked separately)
    assert(report.replayed_steps() == 0);
    assert(report.chain_head().size() == 64);
    const auto recorded_trace = report.trace();

    // Resume: everything comes from the journal — no model calls, and the
    // effect is not re-performed.
    auto resumed = rt.resume("cpp-research-1", agent);
    assert(oracle_calls == 4);
    assert(effect_performs == 1);
    assert(resumed.fresh_steps() == 0);

    // Replay: bit-for-bit trace parity (T1), model never consulted.
    auto audit = rt.replay("cpp-research-1", agent);
    assert(oracle_calls == 4);
    assert(audit.trace() == recorded_trace);

    // The chain verifies end to end.
    assert(rt.verify("cpp-research-1"));

    // Channels: send lands in the inbox, recv journals it.
    rt.send("cpp-approval-1", "approvals", "approved");
    auto approval = rt.run("cpp-approval-1", [](pragmatic::Ctx& ctx) {
        return ctx.recv("approvals");
    });
    assert(approval.output() == "approved");

    // Typed faults surface as FaultError with the right code.
    bool threw = false;
    try {
        rt.run("cpp-fault-1", [](pragmatic::Ctx& ctx) -> std::string {
            ctx.contract(false, "must not continue");
            return "unreachable";
        });
    } catch (const pragmatic::FaultError& e) {
        threw = true;
        assert(e.code() == PRAG_ERR_CONTRACT);
        assert(std::string(e.what()).find("must not continue") != std::string::npos);
    }
    assert(threw);

    // A C++ exception from an agent is caught at the boundary, not UB.
    threw = false;
    try {
        rt.run("cpp-throw-1", [](pragmatic::Ctx&) -> std::string {
            throw std::runtime_error("agent exploded");
        });
    } catch (const pragmatic::FaultError& e) {
        threw = true;
        assert(std::string(e.what()).find("agent exploded") != std::string::npos);
    }
    assert(threw);

    std::printf("cpp smoke: OK (runtime v%s)\n", pragmatic::Runtime::version().c_str());
    fs::remove_all(dir);
    return 0;
}
