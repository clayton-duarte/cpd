// Generated typed pi tool schemas and type definitions (contract §5.9, pro-d514).
// Represents the 12 domain tools over run/core.sock using NDJSON framing.

export interface ToolParameterSchema {
  type: "object";
  properties: Record<string, unknown>;
  required?: string[];
  additionalProperties?: boolean;
}

export interface ToolDefinition {
  name: string;
  domain: string;
  description: string;
  parameters: ToolParameterSchema;
  ops: string[];
}

export const NAMESPACES = [
  "cpd_plan",
  "cpd_job",
  "cpd_workflow",
  "cpd_session",
  "cpd_context",
  "cpd_pr",
  "cpd_secret",
  "cpd_protocol",
  "cpd_policy",
  "cpd_resource",
  "cpd_mcp",
  "cpd_message",
] as const;

export type ToolNamespace = typeof NAMESPACES[number];

export const TOOL_DEFINITIONS: Record<ToolNamespace, ToolDefinition> = {
  cpd_plan: {
    name: "cpd_plan",
    domain: "plan",
    description: "Manage plans: new, show, phase, lock, outline, render, lint, review.",
    ops: ["new", "show", "phase", "lock", "outline", "render", "lint", "review"],
    parameters: {
      type: "object",
      properties: {
        op: { type: "string", enum: ["new", "show", "phase", "lock", "outline", "render", "lint", "review"] },
        params: { type: "object", additionalProperties: true },
      },
      required: ["op"],
    },
  },
  cpd_job: {
    name: "cpd_job",
    domain: "job",
    description: "Manage jobs: add, edit, show, cancel, retry, approve_touches, approve_tests.",
    ops: ["add", "edit", "show", "cancel", "retry", "approve_touches", "approve_tests"],
    parameters: {
      type: "object",
      properties: {
        op: { type: "string", enum: ["add", "edit", "show", "cancel", "retry", "approve_touches", "approve_tests"] },
        params: { type: "object", additionalProperties: true },
      },
      required: ["op"],
    },
  },
  cpd_workflow: {
    name: "cpd_workflow",
    domain: "workflow",
    description: "Manage workflows: add, edit, describe, cancel.",
    ops: ["add", "edit", "describe", "cancel"],
    parameters: {
      type: "object",
      properties: {
        op: { type: "string", enum: ["add", "edit", "describe", "cancel"] },
        params: { type: "object", additionalProperties: true },
      },
      required: ["op"],
    },
  },
  cpd_session: {
    name: "cpd_session",
    domain: "session",
    description: "Manage sessions: show, fork, park, open, close.",
    ops: ["show", "fork", "park", "open", "close"],
    parameters: {
      type: "object",
      properties: {
        op: { type: "string", enum: ["show", "fork", "park", "open", "close"] },
        params: { type: "object", additionalProperties: true },
      },
      required: ["op"],
    },
  },
  cpd_context: {
    name: "cpd_context",
    domain: "context",
    description: "Search, show, decide and note in context.",
    ops: ["search", "show", "decide", "note"],
    parameters: {
      type: "object",
      properties: {
        op: { type: "string", enum: ["search", "show", "decide", "note"] },
        params: { type: "object", additionalProperties: true },
      },
      required: ["op"],
    },
  },
  cpd_pr: {
    name: "cpd_pr",
    domain: "pr",
    description: "Manage PRs: open, view, push, rebase, rerun, adopt, comment, reply.",
    ops: ["open", "view", "push", "rebase", "rerun", "adopt", "comment", "reply"],
    parameters: {
      type: "object",
      properties: {
        op: { type: "string", enum: ["open", "view", "push", "rebase", "rerun", "adopt", "comment", "reply"] },
        params: { type: "object", additionalProperties: true },
      },
      required: ["op"],
    },
  },
  cpd_secret: {
    name: "cpd_secret",
    domain: "secret",
    description: "Manage secrets: get, list, create, edit, delete, allow.",
    ops: ["get", "list", "create", "edit", "delete", "allow"],
    parameters: {
      type: "object",
      properties: {
        op: { type: "string", enum: ["get", "list", "create", "edit", "delete", "allow"] },
        params: { type: "object", additionalProperties: true },
      },
      required: ["op"],
    },
  },
  cpd_protocol: {
    name: "cpd_protocol",
    domain: "protocol",
    description: "Protocol bug commands: bug_report, bug_mitigate, bug_close, bug_show, bug_list.",
    ops: ["bug_report", "bug_mitigate", "bug_close", "bug_show", "bug_list"],
    parameters: {
      type: "object",
      properties: {
        op: { type: "string", enum: ["bug_report", "bug_mitigate", "bug_close", "bug_show", "bug_list"] },
        params: { type: "object", additionalProperties: true },
      },
      required: ["op"],
    },
  },
  cpd_policy: {
    name: "cpd_policy",
    domain: "policy",
    description: "Show policy.",
    ops: ["show"],
    parameters: {
      type: "object",
      properties: {
        op: { type: "string", enum: ["show"] },
        params: { type: "object", additionalProperties: true },
      },
      required: ["op"],
    },
  },
  cpd_resource: {
    name: "cpd_resource",
    domain: "resource",
    description: "List or run resources under lease.",
    ops: ["list", "run"],
    parameters: {
      type: "object",
      properties: {
        op: { type: "string", enum: ["list", "run"] },
        params: { type: "object", additionalProperties: true },
      },
      required: ["op"],
    },
  },
  cpd_mcp: {
    name: "cpd_mcp",
    domain: "mcp",
    description: "Attach MCP server.",
    ops: ["attach"],
    parameters: {
      type: "object",
      properties: {
        op: { type: "string", enum: ["attach"] },
        params: { type: "object", additionalProperties: true },
      },
      required: ["op"],
    },
  },
  cpd_message: {
    name: "cpd_message",
    domain: "message",
    description: "Message bus: delegate (down one tier) and escalate (up one tier to parent).",
    ops: ["delegate", "escalate"],
    parameters: {
      type: "object",
      properties: {
        op: { type: "string", enum: ["delegate", "escalate"] },
        params: { type: "object", additionalProperties: true },
      },
      required: ["op"],
    },
  },
};
