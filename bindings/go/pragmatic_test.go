// Smoke test for the Go binding: record → resume → replay parity, durable
// effects, typed faults, channels, and hash-chain verification — the same
// scenario the Rust, Python, C ABI, C++, and Java tests run.

package pragmatic

import (
	"errors"
	"fmt"
	"reflect"
	"strings"
	"testing"
)

func TestRecordResumeReplay(t *testing.T) {
	dir := t.TempDir()

	oracleCalls := 0
	rt, err := Open(dir, func(prompt string) (string, error) {
		out := fmt.Sprintf("completion#%d(%s)", oracleCalls, prompt)
		oracleCalls++
		return out, nil
	}, []byte("go-smoke-key"))
	if err != nil {
		t.Fatalf("open: %v", err)
	}
	defer rt.Close()

	effectPerforms := 0
	agent := func(ctx *Ctx) (string, error) {
		plan, err := ctx.Oracle("plan the task")
		if err != nil {
			return "", err
		}
		var findings []string
		for step := 0; step < 3; step++ {
			f, err := ctx.Oracle(fmt.Sprintf("probe %d: %s", step, plan))
			if err != nil {
				return "", err
			}
			findings = append(findings, f)
		}
		published, err := ctx.Effect("publish",
			fmt.Sprintf("%d findings", len(findings)),
			func(arg string) (string, error) {
				effectPerforms++
				return "s3://reports/" + arg, nil
			})
		if err != nil {
			return "", err
		}
		return "report(" + published + ")", nil
	}

	// Record.
	report, err := rt.Run("go-research-1", agent)
	if err != nil {
		t.Fatalf("run: %v", err)
	}
	if report.Output != "report(s3://reports/3 findings)" {
		t.Fatalf("output: %q", report.Output)
	}
	if oracleCalls != 4 || effectPerforms != 1 {
		t.Fatalf("calls: oracle=%d effect=%d", oracleCalls, effectPerforms)
	}
	if report.FreshSteps != 4 || report.ReplayedSteps != 0 {
		t.Fatalf("steps: fresh=%d replayed=%d", report.FreshSteps, report.ReplayedSteps)
	}
	if len(report.ChainHead) != 64 {
		t.Fatalf("chain head: %q", report.ChainHead)
	}

	// Resume: everything comes from the journal — no model calls, no
	// re-performed effect.
	resumed, err := rt.Resume("go-research-1", agent)
	if err != nil {
		t.Fatalf("resume: %v", err)
	}
	if oracleCalls != 4 || effectPerforms != 1 {
		t.Fatalf("resume re-sampled: oracle=%d effect=%d", oracleCalls, effectPerforms)
	}
	if resumed.FreshSteps != 0 {
		t.Fatalf("resume fresh: %d", resumed.FreshSteps)
	}

	// Replay: bit-for-bit trace parity (T1), model never consulted.
	audit, err := rt.Replay("go-research-1", agent)
	if err != nil {
		t.Fatalf("replay: %v", err)
	}
	if oracleCalls != 4 {
		t.Fatalf("replay hit the model")
	}
	if !reflect.DeepEqual(audit.Trace, report.Trace) {
		t.Fatalf("trace parity:\nrecorded: %v\nreplayed: %v", report.Trace, audit.Trace)
	}

	// The chain verifies end to end.
	if err := rt.Verify("go-research-1"); err != nil {
		t.Fatalf("verify: %v", err)
	}
}

func TestChannelsAndFaults(t *testing.T) {
	rt, err := Open("", func(prompt string) (string, error) {
		return "ok", nil
	}, nil)
	if err != nil {
		t.Fatalf("open: %v", err)
	}
	defer rt.Close()

	// Channels.
	rt.Send("go-approval-1", "approvals", "approved")
	report, err := rt.Run("go-approval-1", func(ctx *Ctx) (string, error) {
		return ctx.Recv("approvals")
	})
	if err != nil {
		t.Fatalf("recv run: %v", err)
	}
	if report.Output != "approved" {
		t.Fatalf("recv: %q", report.Output)
	}

	// Typed faults surface with the right code.
	_, err = rt.Run("go-fault-1", func(ctx *Ctx) (string, error) {
		if err := ctx.Contract(false, "must not continue"); err != nil {
			return "", err
		}
		return "unreachable", nil
	})
	var fault *Fault
	if !errors.As(err, &fault) {
		t.Fatalf("expected *Fault, got %v", err)
	}
	if fault.Code != ErrContract || !strings.Contains(fault.Message, "must not continue") {
		t.Fatalf("fault: code=%d msg=%q", fault.Code, fault.Message)
	}

	// A plain Go error from an agent comes back as a tool fault.
	_, err = rt.Run("go-error-1", func(ctx *Ctx) (string, error) {
		return "", errors.New("agent exploded")
	})
	if !errors.As(err, &fault) || !strings.Contains(fault.Message, "agent exploded") {
		t.Fatalf("agent error: %v", err)
	}

	if Version() == "" {
		t.Fatal("version")
	}
}
