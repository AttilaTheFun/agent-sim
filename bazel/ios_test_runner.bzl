"""ios_agent_sim_runner: a test runner (rules_apple's AppleTestRunnerInfo) that runs `agent-sim xctest` on the
container's iOS simulator, for `ios_unit_test` and `ios_ui_test` targets built on Linux (rules_applecross)."""

load("@build_bazel_rules_apple//apple:providers.bzl", "apple_provider")

def _ios_agent_sim_runner_impl(ctx):
    template = ctx.actions.declare_file(ctx.label.name + ".template.sh")
    ctx.actions.expand_template(
        template = ctx.file._template,
        output = template,
        substitutions = {
            # rules_apple fills in the test's own substitutions (%(test_bundle_path)s ...) afterwards
            "%(agent_sim)s": ctx.attr.agent_sim,
            "%(x86sim_root)s": ctx.attr.x86sim_root,
            "%(timeout)s": str(ctx.attr.timeout),
        },
    )
    return [
        apple_provider.make_apple_test_runner_info(
            # the phone is a service of the container, outside any sandbox, and never on a remote executor
            execution_requirements = {"no-sandbox": "1", "no-remote-exec": "1"},
            test_environment = ctx.attr.test_environment,
            test_runner_template = template,
        ),
        DefaultInfo(),
    ]

ios_agent_sim_runner = rule(
    implementation = _ios_agent_sim_runner_impl,
    doc = """Runs iOS test bundles with `agent-sim xctest` on the container's simulator (scripts/boot-arm64.sh): logic
and app-hosted unit tests, and UI tests. Tests wait for a free phone.""",
    attrs = {
        "agent_sim": attr.string(
            default = "/usr/local/bin/agent-sim",
            doc = "The agent-sim command (scripts/setup-arm64.sh installs it here).",
        ),
        "x86sim_root": attr.string(
            default = "/root/x86sim",
            doc = "x86Sim's checkout the phone runs from ($WORK/x86sim of scripts/setup-arm64.sh). " +
                  "`bazel test` gives tests their own HOME, so it is not found from there.",
        ),
        "timeout": attr.int(default = 1800, doc = "Seconds per test target."),
        "test_environment": attr.string_dict(doc = "Environment variables for the test action."),
        "_template": attr.label(
            default = "ios_test_runner.template.sh",
            allow_single_file = True,
        ),
    },
)
