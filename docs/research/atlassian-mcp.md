# Atlassian MCP: the Jira tools cpd's transition helper uses

Probed for real in Job `probe-docs` (attempt `probe-docs-a1`, Plan `jira-sync`) on 2026-10-27, through the same connection the helper will use (`McpServerConnection` + `McpOAuthCredentialStore` from pi's `extensions/mcp`, as `pi/lib/slack-channel.ts` `connect` does). Every response below came from a real call made in this Job. Redactions: `<site>`, `<cloud-id>`, `<account-id>`, `<name>`, `<avatar-url>`; status, transition and field names are verbatim. In responses, `rateLimit.headers` is elided (rate-limit counters only; the rest of `rateLimit` is kept). Scratch issue: `EEC-3413` (project EEC), the only issue written, only its status, and only into `In Review`, `In Progress` and back to `BLOCKED`.

Each sample is preceded by `isError`, the content-block types and the wall time of that call, taken from the `CallToolResult`.

## 1. Server and sign-in

Server `atlassian`, url `https://mcp.atlassian.com/v1/mcp/authv2`, exposure `deferred`, OAuth through pi's own store. The connection reached state `connected` (read from `conn.state` after the calls).

Finding: the entry in `~/.pi/agent/mcp.json` has `"enabled": false`, so `timeout 60 pi mcp list` prints `atlassian: disabled (deferred, global)` and `pi mcp list --json` gives `"state": "disabled", "tools": []`. The probe connects anyway because it passes `{ ...entry, enabled: true }`. The helper must do the same (or the entry must be enabled), otherwise it sees no tools. The sign-in itself is valid (state `connected`).

Exact tool list of the server (41 tools, read from the connection's `tools`):

`atlassianUserInfo`, `getAccessibleAtlassianResources`, `getContentFormatGuide`, `getConfluencePage`, `searchConfluenceUsingCql`, `getConfluenceSpaces`, `getPagesInConfluenceSpace`, `getConfluencePageFooterComments`, `getConfluencePageInlineComments`, `getConfluenceCommentChildren`, `getConfluencePageDescendants`, `createConfluencePage`, `updateConfluencePage`, `createConfluenceFooterComment`, `createConfluenceInlineComment`, `getJiraIssue`, `editJiraIssue`, `createJiraIssue`, `getTransitionsForJiraIssue`, `getJiraIssueRemoteIssueLinks`, `getVisibleJiraProjects`, `getJiraProjectIssueTypesMetadata`, `getJiraIssueTypeMetaWithFields`, `addCommentToJiraIssue`, `transitionJiraIssue`, `searchJiraIssuesUsingJql`, `lookupJiraAccountId`, `addWorklogToJiraIssue`, `getIssueLinkTypes`, `createIssueLink`, `getCompassComponents`, `getCompassComponent`, `getCompassCustomFieldDefinitions`, `createCompassCustomFieldDefinition`, `createCompassComponent`, `createCompassComponentRelationship`, `getTeamworkGraphContext`, `getTeamworkGraphObject`, `addTeamworkGraphContext`, `search`, `fetch`

The four tools the helper uses: `getAccessibleAtlassianResources`, `getJiraIssue`, `getTransitionsForJiraIssue`, `transitionJiraIssue`. Their annotations: the first three `readOnlyHint: true`; `transitionJiraIssue` `readOnlyHint: false`, `destructiveHint: false`, `idempotentHint: true`.

Every response of these tools is ONE content block of type `text`, whose text is JSON (pretty-printed, 2 spaces) on success and on tool errors, except input-schema errors (section 6), which are plain text. No `structuredContent` was returned.

## 2. Site

Source: real calls in this Job.

Every Jira tool takes a required `cloudId` whose schema description is `Cloud ID (UUID or site URL)`. Obtain it with `getAccessibleAtlassianResources {}` (about 1.0 s for the whole process, `isError: false`, one `text` block):

```json
[
  {
    "id": "<cloud-id>",
    "url": "<site>",
    "name": "<name>",
    "scopes": [
      "read:comment:confluence",
      "read:confluence-user",
      "read:page:confluence",
      "read:space:confluence",
      "search:confluence",
      "write:comment:confluence",
      "write:page:confluence"
    ],
    "avatarUrl": "<avatar-url>"
  },
  {
    "id": "<cloud-id>",
    "url": "<site>",
    "name": "<name>",
    "scopes": [
      "read:jira-work",
      "write:jira-work"
    ],
    "avatarUrl": "<avatar-url>"
  }
]
```

- The array has two entries for the one site (same `id`, same `url`, different `scopes`): one with the Confluence scopes, one with `read:jira-work` and `write:jira-work`. Only ONE site is accessible, so a user with more than one site is UNTESTED. Pick the entry whose `scopes` include `read:jira-work` and `write:jira-work`, and use its `id`.
- A site URL replaces the cloudId: `getJiraIssue` with `cloudId` set to the site URL (`<site>`) returned the same issue as the UUID (read for real, `isError: false`). VERIFIED for `getJiraIssue`; the schema text is the same on `getTransitionsForJiraIssue` and `transitionJiraIssue`, but only the UUID was used on those two (the URL form is UNTESTED there).
- A UUID not among the accessible resources, or a host with no site, is refused (section 6).
- Manager's fact "getAccessibleAtlassianResources {} -> a cloudId for the missionlane site": VERIFIED (the response has `id` and `url`; two entries, see above).

## 3. Reading an issue

Source: real calls in this Job.

Tool `getJiraIssue`. Required: `cloudId`, `issueIdOrKey` (key like `PROJ-123` or numeric id). Optional: `fields` (array of strings; when omitted or empty it defaults to summary, description, status, issuetype, priority, labels, components, assignee, reporter, created, updated, resolution, project; `"*all"` returns everything), `fieldsByKeys`, `expand`, `properties`, `updateHistory`, `failFast`, `responseContentFormat` (`markdown` | `adf`), `actionSource`.

The response can be limited to the status: `fields: ["status"]` returns the least that carries the status name AND the statusCategory. Real response for `EEC-3413` with `{"cloudId":"<cloud-id>","issueIdOrKey":"EEC-3413","fields":["status"]}`: isError: `false`, content blocks: `["text"]`, call time 930 ms

```json
{
  "expand": "renderedFields,names,schema,operations,editmeta,changelog,versionedRepresentations",
  "id": "255705",
  "self": "https://api.atlassian.com/ex/jira/<cloud-id>/rest/api/3/issue/255705",
  "key": "EEC-3413",
  "fields": {
    "status": {
      "self": "https://api.atlassian.com/ex/jira/<cloud-id>/rest/api/3/status/11579",
      "description": "",
      "iconUrl": "https://api.atlassian.com/ex/jira/<cloud-id>/",
      "name": "BLOCKED",
      "id": "11579",
      "statusCategory": {
        "self": "https://api.atlassian.com/ex/jira/<cloud-id>/rest/api/3/statuscategory/2",
        "id": 2,
        "key": "new",
        "colorName": "blue-gray",
        "name": "To Do"
      }
    }
  }
}
```

- Status name: `fields.status.name` (`BLOCKED` here). Category: `fields.status.statusCategory.key` (`new`) and `.name` (`To Do`). So the statusCategory DOES come with the status; a Done-category ticket shows `key: "done"`, `name: "Done"` (seen in `to.statusCategory` of the transitions, section 4; not seen on an issue because no issue was moved to Done).
- The body is JSON text, in the first (and only) content block, type `text`. Top-level keys with `fields: ["status"]`: `expand`, `id`, `self`, `key`, `fields`. The `expand` value is a fixed string, not a request.
- `fields: ["statusCategory"]` returns the category alone, without the status name: isError: `false`, content blocks: `["text"]`, call time 881 ms

```json
{
  "expand": "renderedFields,names,schema,operations,editmeta,changelog,versionedRepresentations",
  "id": "255705",
  "self": "https://api.atlassian.com/ex/jira/<cloud-id>/rest/api/3/issue/255705",
  "key": "EEC-3413",
  "fields": {
    "statusCategory": {
      "self": "https://api.atlassian.com/ex/jira/<cloud-id>/rest/api/3/statuscategory/2",
      "id": 2,
      "key": "new",
      "colorName": "blue-gray",
      "name": "To Do"
    }
  }
}
```

- Without `fields` the default field set is returned (includes `status`, with the same `statusCategory`); real call, `isError: false`.
- Manager's fact "getJiraIssue {cloudId, issueIdOrKey, fields:["status"]} -> fields.status.name": VERIFIED. Its open question "does the status carry statusCategory": YES, `key` and `name` (also `id` and `colorName`).

## 4. Reading transitions

Source: real calls in this Job (the issue stood in `BLOCKED`, then `In Review`, then `In Progress`, read before each move).

Tool `getTransitionsForJiraIssue`. Required: `cloudId`, `issueIdOrKey`. Optional: `expand`, `transitionId`, `skipRemoteOnlyCondition`, `includeUnavailableTransitions`, `sortByOpsBarAndStatus`, `actionSource`. Default call, from `BLOCKED`: isError: `false`, content blocks: `["text"]`, call time 894 ms

```json
{
  "expand": "transitions",
  "transitions": [
    {
      "id": "2",
      "name": "In Review",
      "to": {
        "self": "https://api.atlassian.com/ex/jira/<cloud-id>/rest/api/3/status/11572",
        "description": "",
        "iconUrl": "https://api.atlassian.com/ex/jira/<cloud-id>/",
        "name": "In Review",
        "id": "11572",
        "statusCategory": {
          "self": "https://api.atlassian.com/ex/jira/<cloud-id>/rest/api/3/statuscategory/4",
          "id": 4,
          "key": "indeterminate",
          "colorName": "yellow",
          "name": "In Progress"
        }
      },
      "hasScreen": false,
      "isGlobal": true,
      "isInitial": false,
      "isAvailable": true,
      "isConditional": false,
      "isLooped": false
    },
    {
      "id": "3",
      "name": "BLOCKED",
      "to": {
        "self": "https://api.atlassian.com/ex/jira/<cloud-id>/rest/api/3/status/11579",
        "description": "",
        "iconUrl": "https://api.atlassian.com/ex/jira/<cloud-id>/",
        "name": "BLOCKED",
        "id": "11579",
        "statusCategory": {
          "self": "https://api.atlassian.com/ex/jira/<cloud-id>/rest/api/3/statuscategory/2",
          "id": 2,
          "key": "new",
          "colorName": "blue-gray",
          "name": "To Do"
        }
      },
      "hasScreen": false,
      "isGlobal": true,
      "isInitial": false,
      "isAvailable": true,
      "isConditional": false,
      "isLooped": false
    },
    {
      "id": "8",
      "name": "Won't Do",
      "to": {
        "self": "https://api.atlassian.com/ex/jira/<cloud-id>/rest/api/3/status/11598",
        "description": "",
        "iconUrl": "https://api.atlassian.com/ex/jira/<cloud-id>/",
        "name": "Won't Do",
        "id": "11598",
        "statusCategory": {
          "self": "https://api.atlassian.com/ex/jira/<cloud-id>/rest/api/3/statuscategory/3",
          "id": 3,
          "key": "done",
          "colorName": "green",
          "name": "Done"
        }
      },
      "hasScreen": false,
      "isGlobal": true,
      "isInitial": false,
      "isAvailable": true,
      "isConditional": false,
      "isLooped": false
    },
    {
      "id": "9",
      "name": "Backlog",
      "to": {
        "self": "https://api.atlassian.com/ex/jira/<cloud-id>/rest/api/3/status/11552",
        "description": "",
        "iconUrl": "https://api.atlassian.com/ex/jira/<cloud-id>/",
        "name": "Backlog",
        "id": "11552",
        "statusCategory": {
          "self": "https://api.atlassian.com/ex/jira/<cloud-id>/rest/api/3/statuscategory/2",
          "id": 2,
          "key": "new",
          "colorName": "blue-gray",
          "name": "To Do"
        }
      },
      "hasScreen": false,
      "isGlobal": true,
      "isInitial": false,
      "isAvailable": true,
      "isConditional": false,
      "isLooped": false
    },
    {
      "id": "10",
      "name": "Ready to Groom",
      "to": {
        "self": "https://api.atlassian.com/ex/jira/<cloud-id>/rest/api/3/status/14757",
        "description": "",
        "iconUrl": "https://api.atlassian.com/ex/jira/<cloud-id>/",
        "name": "Ready to Groom",
        "id": "14757",
        "statusCategory": {
          "self": "https://api.atlassian.com/ex/jira/<cloud-id>/rest/api/3/statuscategory/2",
          "id": 2,
          "key": "new",
          "colorName": "blue-gray",
          "name": "To Do"
        }
      },
      "hasScreen": false,
      "isGlobal": true,
      "isInitial": false,
      "isAvailable": true,
      "isConditional": false,
      "isLooped": false
    },
    {
      "id": "11",
      "name": "Ready for Dev",
      "to": {
        "self": "https://api.atlassian.com/ex/jira/<cloud-id>/rest/api/3/status/14756",
        "description": "",
        "iconUrl": "https://api.atlassian.com/ex/jira/<cloud-id>/",
        "name": "Ready for Dev",
        "id": "14756",
        "statusCategory": {
          "self": "https://api.atlassian.com/ex/jira/<cloud-id>/rest/api/3/statuscategory/2",
          "id": 2,
          "key": "new",
          "colorName": "blue-gray",
          "name": "To Do"
        }
      },
      "hasScreen": false,
      "isGlobal": true,
      "isInitial": false,
      "isAvailable": true,
      "isConditional": false,
      "isLooped": false
    },
    {
      "id": "21",
      "name": "In Progress",
      "to": {
        "self": "https://api.atlassian.com/ex/jira/<cloud-id>/rest/api/3/status/11553",
        "description": "This work item is being actively worked on at the moment by the assignee.",
        "iconUrl": "https://api.atlassian.com/ex/jira/<cloud-id>/",
        "name": "In Progress",
        "id": "11553",
        "statusCategory": {
          "self": "https://api.atlassian.com/ex/jira/<cloud-id>/rest/api/3/statuscategory/4",
          "id": 4,
          "key": "indeterminate",
          "colorName": "yellow",
          "name": "In Progress"
        }
      },
      "hasScreen": false,
      "isGlobal": true,
      "isInitial": false,
      "isAvailable": true,
      "isConditional": false,
      "isLooped": false
    },
    {
      "id": "31",
      "name": "Done",
      "to": {
        "self": "https://api.atlassian.com/ex/jira/<cloud-id>/rest/api/3/status/11554",
        "description": "",
        "iconUrl": "https://api.atlassian.com/ex/jira/<cloud-id>/",
        "name": "Done",
        "id": "11554",
        "statusCategory": {
          "self": "https://api.atlassian.com/ex/jira/<cloud-id>/rest/api/3/statuscategory/3",
          "id": 3,
          "key": "done",
          "colorName": "green",
          "name": "Done"
        }
      },
      "hasScreen": false,
      "isGlobal": true,
      "isInitial": false,
      "isAvailable": true,
      "isConditional": false,
      "isLooped": false
    }
  ]
}
```

(`PR Approved` does not appear from `BLOCKED`; see the `In Review` table below.) Each entry has `id` (a string of an integer), `name`, `to` (the target status: `name`, `id`, `statusCategory` with `key` and `name`), and flags `hasScreen`, `isGlobal`, `isInitial`, `isAvailable`, `isConditional`, `isLooped`. The top-level `expand` is `"transitions"`.

Transitions out of `BLOCKED` (summarised from the response above; the three other tables are summarised the same way from their own real calls):

| id | name | to.name | to.statusCategory (key / name) |
|---|---|---|---|
| 2 | In Review | In Review | `indeterminate` / In Progress |
| 3 | BLOCKED | BLOCKED | `new` / To Do |
| 8 | Won't Do | Won't Do | `done` / Done |
| 9 | Backlog | Backlog | `new` / To Do |
| 10 | Ready to Groom | Ready to Groom | `new` / To Do |
| 11 | Ready for Dev | Ready for Dev | `new` / To Do |
| 21 | In Progress | In Progress | `indeterminate` / In Progress |
| 31 | Done | Done | `done` / Done |

Transitions out of `In Review`:

| id | name | to.name | to.statusCategory (key / name) |
|---|---|---|---|
| 2 | In Review | In Review | `indeterminate` / In Progress |
| 3 | BLOCKED | BLOCKED | `new` / To Do |
| 8 | Won't Do | Won't Do | `done` / Done |
| 9 | Backlog | Backlog | `new` / To Do |
| 10 | Ready to Groom | Ready to Groom | `new` / To Do |
| 11 | Ready for Dev | Ready for Dev | `new` / To Do |
| 21 | In Progress | In Progress | `indeterminate` / In Progress |
| 31 | Done | Done | `done` / Done |
| 6 | PR Approved | Done | `done` / Done |

Transitions out of `In Progress`:

| id | name | to.name | to.statusCategory (key / name) |
|---|---|---|---|
| 2 | In Review | In Review | `indeterminate` / In Progress |
| 3 | BLOCKED | BLOCKED | `new` / To Do |
| 8 | Won't Do | Won't Do | `done` / Done |
| 9 | Backlog | Backlog | `new` / To Do |
| 10 | Ready to Groom | Ready to Groom | `new` / To Do |
| 11 | Ready for Dev | Ready for Dev | `new` / To Do |
| 21 | In Progress | In Progress | `indeterminate` / In Progress |
| 31 | Done | Done | `done` / Done |

- The status names the workflow shows: `BLOCKED`, `In Review`, `In Progress`, `Done`, `Won't Do`, `Backlog`, `Ready to Groom`, `Ready for Dev`. The real names ARE exactly `In Progress`, `In Review`, `Done` (the `statusCategory` of `In Progress` and `In Review` is `indeterminate` / `In Progress`; of `Done` and `Won't Do` it is `done` / `Done`; of `BLOCKED`, `Backlog`, `Ready to Groom`, `Ready for Dev` it is `new` / `To Do`).
- `In Review` is reachable directly from `BLOCKED` (id 2, taken for real) and from `In Progress` (id 2, offered in the list).
- `Done` is offered from `In Review`: transition id 31 `Done` and also id 6 `PR Approved`, whose `to` is `Done` (read from the list; never taken). So the transition NAME is not the target name: `PR Approved` leads to `Done`. Pick by `to.name`, never by `name`.
- The list includes the transition back to the status the issue is already in (from `In Review`: id 2 `In Review`; from `In Progress`: id 21 `In Progress`; from `BLOCKED`: id 3 `BLOCKED`). All entries had `isAvailable: true`, `isConditional: false`, `isLooped: false` here. The helper must skip a transition whose target is the current status.
- Manager's fact "transitions[] each with id, name, to.name (from BLOCKED in project EEC: 2 In Review, 3 BLOCKED, 8 Won't Do, 9 Backlog, 10 Ready to Groom, 11 Ready for Dev, 21 In Progress, 31 Done)": VERIFIED, every id and name matches; `to.name` equals `name` for all of them in this list, except that from `In Review` the extra entry id 6 `PR Approved` has `to.name` `Done`.
- A count of 8 transitions came back with `includeUnavailableTransitions: true` from `BLOCKED`, the same as without it; the option changed nothing here.

## 5. Transitioning

Source: real calls in this Job. `EEC-3413` was read before each one.

Tool `transitionJiraIssue`. Required: `cloudId`, `issueIdOrKey`, `transition` (object, only key `id`, a string that must be an integer, `additionalProperties: false`). Optional: `fields`, `update`, `historyMetadata`, `actionSource`. No other field is required for these moves. A transition is chosen by ID only: not by name and not by target status (section 6 shows both refused). The id comes from `getTransitionsForJiraIssue`, matched by `to.name`.

The three moves, each preceded by a status read (`BLOCKED`, `In Review`, `In Progress`) and followed by one that showed the new status:

1. `BLOCKED` to `In Review`, `transition: {"id":"2"}`: isError: `false`, content blocks: `["text"]`, call time 1577 ms

```json
{
  "success": true,
  "statusName": "In Review"
}
```

2. `In Review` to `In Progress`, `transition: {"id":"21"}`: isError: `false`, content blocks: `["text"]`, call time 1871 ms

```json
{
  "success": true,
  "statusName": "In Progress"
}
```

3. `In Progress` to `BLOCKED`, `transition: {"id":"3"}`: isError: `false`, content blocks: `["text"]`, call time 1576 ms

```json
{
  "success": true,
  "statusName": "BLOCKED"
}
```

The response is `{"success": true, "statusName": "<new status name>"}`, one `text` block. `statusName` is the status the issue is in after the move. Manager's fact "transitionJiraIssue {cloudId, issueIdOrKey, transition:{id}} -> {success:true, statusName}": VERIFIED.

Taking the transition whose target is the current status is NOT refused: with the issue in `In Review`, `transition: {"id":"2"}` returned: isError: `false`, content blocks: `["text"]`, call time 1971 ms

```json
{
  "success": true,
  "statusName": "In Review"
}
```

(The same happened for `In Progress` and id 21: `{"success": true, "statusName": "In Progress"}`.) So moving to the status the issue is already in is a successful no-op; the helper should not depend on a refusal for it, and should skip it.

## 6. Refusals

Source: real calls in this Job. All against `EEC-3413` or the unknown key shown.

**Unknown issue key** (`EEC-99999999`; `getJiraIssue`, `getTransitionsForJiraIssue` and `transitionJiraIssue` all gave this shape, `statusCode` 404; the `transitionJiraIssue` message begins `Failed to transition issue: Not Found. `): isError: `true`, content blocks: `["text"]`, call time 1273 ms

```json
{
  "error": true,
  "message": "{\"errorMessages\":[\"Issue does not exist or you do not have permission to see it.\"],\"errors\":{}}",
  "rateLimit": {
    "limit": 400,
    "remaining": 399
  },
  "statusCode": 404
}
```

The text says "does not exist or you do not have permission", so a missing issue and a missing permission on it are the same refusal. Permanent for a retry (the key will not appear by itself); park.

**A transition id not available from the current status** (`id: "999"` while the issue stood in `In Review`; the issue's status was read back unchanged): isError: `true`, content blocks: `["text"]`, call time 1142 ms

```json
{
  "error": true,
  "message": "Failed to transition issue: Bad Request. {\"errorMessages\":[\"Transition id '999' is not valid for this issue.\"],\"errors\":{}}",
  "repairHint": "Fix each entry in problems. Fields with canOmit:true can be dropped if you do not have a valid value; otherwise provide a valid value (use allowedValues where present). Then retry.",
  "problems": [
    {
      "field": "transition",
      "name": "Transition",
      "reason": "Transition id '999' is not valid for this issue.",
      "canOmit": false,
      "allowedValues": [
        {
          "id": "2",
          "value": "In Review"
        },
        {
          "id": "3",
          "value": "BLOCKED"
        },
        {
          "id": "8",
          "value": "Won't Do"
        },
        {
          "id": "9",
          "value": "Backlog"
        },
        {
          "id": "10",
          "value": "Ready to Groom"
        },
        {
          "id": "11",
          "value": "Ready for Dev"
        },
        {
          "id": "21",
          "value": "In Progress"
        },
        {
          "id": "31",
          "value": "Done"
        }
      ]
    }
  ],
  "rateLimit": {
    "limit": 200,
    "remaining": 199
  },
  "statusCode": 400
}
```

`problems[0].allowedValues` lists the ids the issue can take. Permanent for that issue state; re-read the transitions and decide again, do not retry the same id. (The `allowedValues` list had `{"id":"6","value":"PR Approved"}` as the ninth entry from `In Review`.)

**A transition chosen by name, or by a target status name as `id`** (`transition: {"id":"In Progress"}`): isError: `true`, content blocks: `["text"]`, call time 1025 ms

```json
{
  "error": true,
  "message": "Failed to transition issue: Bad Request. {\"errorMessages\":[\"'transition' identifier must be an integer\"],\"errors\":{}}",
  "repairHint": "Fix each entry in problems. Fields with canOmit:true can be dropped if you do not have a valid value; otherwise provide a valid value (use allowedValues where present). Then retry.",
  "problems": [
    {
      "field": "transition",
      "name": "Transition",
      "reason": "'transition' identifier must be an integer",
      "canOmit": false,
      "allowedValues": [
        {
          "id": "2",
          "value": "In Review"
        },
        {
          "id": "3",
          "value": "BLOCKED"
        },
        {
          "id": "8",
          "value": "Won't Do"
        },
        {
          "id": "9",
          "value": "Backlog"
        },
        {
          "id": "10",
          "value": "Ready to Groom"
        },
        {
          "id": "11",
          "value": "Ready for Dev"
        },
        {
          "id": "21",
          "value": "In Progress"
        },
        {
          "id": "31",
          "value": "Done"
        }
      ]
    }
  ],
  "rateLimit": {
    "limit": 200,
    "remaining": 199
  },
  "statusCode": 400
}
```

**A transition with `name` instead of `id`** (`transition: {"name":"In Progress"}`), refused by the input schema before the Jira call: isError: `true`, content blocks: `["text"]`, call time 684 ms

```text
MCP error -32602: Input validation error: Invalid arguments for tool transitionJiraIssue: Required at transition.id
```

Both are permanent (a bug in the caller, not a state).

**A site (cloudId) that does not exist** (`00000000-0000-0000-0000-000000000000`; the same text for `getJiraIssue`, `getTransitionsForJiraIssue`, `transitionJiraIssue`): isError: `true`, content blocks: `["text"]`, call time 648 ms

```json
{
  "error": true,
  "message": "Cloud id: 00000000-0000-0000-0000-000000000000 isn't explicitly granted by the user."
}
```

**A site URL that has no site**: isError: `true`, content blocks: `["text"]`, call time 649 ms

```json
{
  "error": true,
  "message": "No cloud ID found for hostname: <other-host>"
}
```

Both are permanent (the site list does not change on a retry); park.

**A missing required argument** (`cloudId` omitted), refused by the input schema as plain text, not JSON: isError: `true`, content blocks: `["text"]`, call time 646 ms

```text
MCP error -32602: Input validation error: Invalid arguments for tool getJiraIssue: Required at cloudId
```

Permanent (caller bug).

**A call without permission**: NOT PROVOKED. The only safe provocation is an issue or site the signed-in user cannot see, and Jira answers both as the unknown-key text above ("does not exist or you do not have permission"), and a cloudId the user did not grant as "isn't explicitly granted by the user". No call with a missing scope was made (the granted scopes could not be reduced and no write to another issue was allowed). A real 401/403 shape is therefore unobserved.

The `rateLimit` object in the JSON errors carried `limit` values 200 (writes), 350 and 400 (reads) and `remaining` one or two below it.

## 7. Auth and outages

Source: observed in this Job; what could not be observed is listed.

- Signed in (now): `conn.state` is `connected` after the calls; `pi mcp list` shows `atlassian: disabled (deferred, global)` only because `enabled` is `false` in `mcp.json` (section 1), not because of auth.
- Sign-in missing or expired: NOT OBSERVED (the User signed in and this Job may not sign out). Read from pi's source (`extensions/mcp/runtime.js` and `cli.js`, not from a run, so unverified by a call): the connection state becomes `needs-auth` and the call throws `Error` with the message `MCP server "atlassian" requires sign-in. Run /mcp to sign in.` (`/login <provider>` instead of `/mcp` when the config has an `auth.provider`); `pi mcp list` then prints `atlassian: needs sign-in (deferred, global)` followed by `  sign in with: pi mcp login atlassian`, and `pi mcp list --json` gives `"state": "needs-auth"`. A failed connection (not auth) has state `failed` and the message `MCP server "atlassian" failed to connect: <reason>`. Needs-auth only a human clears: permanent for the helper, park, do not retry in a loop.
- Time of a typical call, including a fresh connection per process: `getAccessibleAtlassianResources` about 1.0 s for the whole `node` process; per call (the `ms` shown above): reads 0.6 to 1.1 s, `getTransitionsForJiraIssue` 0.9 s, `transitionJiraIssue` 1.0 to 2.0 s (success 1.6 to 2.0 s), errors 0.6 to 1.1 s. The probe used a 20 s per-call timeout; none came near it.

## 8. Verdict for the helper

Minimal sequence, for ONE issue key `K`, wanting target status name `T` (`In Progress`, `In Review`, `Done`):

1. Once per process (cache): `getAccessibleAtlassianResources {}`; take the entry whose `scopes` include `read:jira-work` and `write:jira-work`; `cloudId` = its `id`. (Or pass the site URL as `cloudId`, verified for `getJiraIssue` only.)
2. `getJiraIssue {"cloudId": <cloudId>, "issueIdOrKey": K, "fields": ["status"]}`; read `fields.status.name` and `fields.status.statusCategory.key`. If the name is already `T`, stop (done). If the category key is `done` and `T` is not `Done`, leave the ticket alone.
3. `getTransitionsForJiraIssue {"cloudId": <cloudId>, "issueIdOrKey": K}`; find the entry whose `to.name` equals `T` (exact match; ignore `name`, e.g. `PR Approved` leads to `Done`). None found: permanent, park (the workflow has no path from this status).
4. `transitionJiraIssue {"cloudId": <cloudId>, "issueIdOrKey": K, "transition": {"id": <that entry's id>}}`; success is `isError: false` and `success: true`; `statusName` is the new status.
5. Optional proof: repeat step 2 and check `fields.status.name` equals `T`.

Caveat for a human-visible side effect: `In Review` offers `Done` through two transitions (id 31 `Done`, id 6 `PR Approved`), both to `Done`; a helper moving to `Done` takes the first match by `to.name`.

Mapping of the refusals of section 6 (rule: `isError: true` with a JSON `statusCode` of 4xx other than 429 or an `MCP error -32602` text is permanent; a transport failure, a 5xx or a 429 is transient; the last two were not observed):

| Refusal | Class |
|---|---|
| unknown issue key (404, "does not exist or you do not have permission") | permanent (park) |
| transition id not available from the current status (400, `Transition id '...' is not valid for this issue.`) | permanent for that state: re-read the transitions once, then park |
| transition `id` not an integer (400) / `name` instead of `id` or missing `cloudId` (`MCP error -32602`) | permanent (caller bug) |
| cloudId that does not exist (`Cloud id: ... isn't explicitly granted by the user.`) | permanent (park) |
| site URL with no site (`No cloud ID found for hostname: ...`) | permanent (park) |
| without permission | not provoked; Jira words it as the unknown-key text, so permanent |
| sign-in missing or expired (`needs-auth`, from source) | permanent until the User signs in: park |
| connection `failed`, timeout, 5xx, 429 (not observed) | transient: retry with backoff |

Final state: `EEC-3413` was read back in status `BLOCKED` after the last write (third move, `In Progress` to `BLOCKED`), and again after the last probe call of this Job: `BLOCKED`.

## 9. The helper against the real server

Run in Job `sync-verify` (attempt `sync-verify-a1`, Plan `jira-sync`) on 2026-10-28 EDT: the real `pi/lib/jira-transition.ts`, once per case, through the connection built from the machine policy's server object (not from `mcp.json`), with the sign-in of the User.

Scratch ticket: the ticket named in the Spec, EEC-3413, was deleted by the User before this Job (the pre-write read answered 404 "Issue does not exist or you do not have permission to see it", no write made). By the User's decision, relayed by the Lead, this Job created ONE new scratch ticket, **EEC-3427**, with `createJiraIssue` (`projectKey` `EEC`, `issueTypeName` `Task`, summary `cpd jira-sync verify scratch: safe to delete`, nothing else) for this check. Its initial status, read at once with `getJiraIssue` `fields: ["status"]`, was `Backlog`. The only writes after the creation were status transitions of EEC-3427 (by the helper into `In Review`; the restore below); none into `Done` or `Won't Do`.

Every request below used `names` = `{"in_progress": "In Progress", "in_review": "In Review", "done": "Done"}` unless said, and `server` = `{"url": "https://mcp.atlassian.com/v1/mcp/authv2", "exposure": "deferred"}` unless said. Command: `printf '%s' '<request>' | PI_CODING_AGENT_DIR="$HOME/.pi/agent" timeout 90 node pi/lib/jira-transition.ts --coding-agent <entry>` with the entry of pi 1.3.0.

### 9.1 Run 1: target `in_review` from `Backlog`

Request: `{"server": <server>, "ticket": "EEC-3427", "target": "in_review", "names": <names>}`

Output (exit status 0):

```
{"ok":true,"ticket":"EEC-3427","target":"in_review","outcome":"moved","from":"Backlog","to":"In Review"}
```

Expected `moved` into In Review, exit 0 (from `Backlog` instead of `BLOCKED`, the new ticket's initial status): VERIFIED.

### 9.2 Run 2: the same request again

Request: as 9.1.

Output (exit status 0):

```
{"ok":true,"ticket":"EEC-3427","target":"in_review","outcome":"unchanged","status":"In Review","why":"at_target"}
```

Expected `unchanged` why `at_target`: VERIFIED.

### 9.3 Run 3: target `in_progress`

Request: `{"server": <server>, "ticket": "EEC-3427", "target": "in_progress", "names": <names>}`

Output (exit status 0):

```
{"ok":true,"ticket":"EEC-3427","target":"in_progress","outcome":"unchanged","status":"In Review","why":"past_target"}
```

Expected `unchanged` why `past_target`: VERIFIED.

### 9.4 Run 4: `names.in_review` = `Nope`, target `in_review`

Request: `{"server": <server>, "ticket": "EEC-3427", "target": "in_review", "names": {"in_progress": "In Progress", "in_review": "Nope", "done": "Done"}}` (the ticket stood in `In Review`, not `Nope`).

Output (exit status 1):

```
{"ok":false,"error":{"code":"jira_refused","message":"EEC-3427 has no transition to \"Nope\" from \"In Review\".","details":{"reason":"no_transition"}}}
```

Expected `jira_refused` reason `no_transition`, exit 1, no move (the status was read back as `In Review` before the restore): VERIFIED.

### 9.5 Run 5: ticket EEC-99999999

Request: `{"server": <server>, "ticket": "EEC-99999999", "target": "in_review", "names": <names>}`

Output (exit status 1):

```
{"ok":false,"error":{"code":"jira_refused","message":"getJiraIssue: the issue was not found.","details":{"tool":"getJiraIssue","reason":"issue_not_found"}}}
```

Expected `jira_refused` reason `issue_not_found`, exit 1: VERIFIED.

### 9.6 Run 6: server url `https://example.invalid/mcp`

Request: `{"server": {"url": "https://example.invalid/mcp"}, "ticket": "EEC-3427", "target": "in_review", "names": <names>}`

Output (exit status 1, about 1 s, no hang):

```
{"ok":false,"error":{"code":"jira_failed","message":"getAccessibleAtlassianResources failed: MCP server \"atlassian\" failed to connect: fetch failed","details":{"tool":"getAccessibleAtlassianResources"}}}
```

Expected a non-zero exit with a one-line JSON failure (any `jira_*` code) and no hang past 90 s: VERIFIED; the code is `jira_failed`.

### 9.7 Run 7: restore

`Backlog` (the initial status) is not a target of the helper, so EEC-3427 was moved back through the same connection (a short node probe, not the helper): `getTransitionsForJiraIssue` listed `Backlog` as id 9 (`to.name` `Backlog`), then `transitionJiraIssue` with `{"cloudId": "<cloud-id>", "issueIdOrKey": "EEC-3427", "transition": {"id": "9"}}`.

Output (isError `false`):

```
{"success": true, "statusName": "Backlog"}
```

Read back with `getJiraIssue` `fields: ["status"]`: `Backlog`. Expected the ticket back in its initial status: VERIFIED.

Summary: all seven runs matched the expectation; nothing differs and the helper needs no fix. The scratch ticket EEC-3427 was read back in status `Backlog` after the last write. The scratch ticket EEC-3427 was left in Backlog; the MCP has no delete tool, so it is for the User to delete.
