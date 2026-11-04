import type { ReactElement } from "react";
import type { CockpitAction, JobView, PlanViewModel } from "../../protocol/cockpit.ts";

function Job(props: { job: JobView }): ReactElement {
  const { job } = props;
  return (
    <li data-job={job.key} data-status={job.status}>
      <span>{job.key}</span> <span>{job.title}</span> <span>{job.status}</span> <span>{job.tier}</span>
      {job.needs.length > 0 ? <span> needs: {job.needs.join(", ")}</span> : null}
      {job.attempts.map((a) => (
        <span key={a.id}> #{a.number} {a.status}</span>
      ))}
    </li>
  );
}

export function PlanView(props: { plan: PlanViewModel | null; onAction(action: CockpitAction): void;
  onRetryJob?: (jobKey: string, workflowKey: string) => void;
}): ReactElement {
  const { plan, onAction } = props;
  if (plan === null) return <p data-empty>No plan selected</p>;
  const explore = plan.jobs.filter((j) => j.workflow === null);
  return (
    <section aria-label={`Plan ${plan.planId}`}>
      <h2>{plan.title}</h2>
      <p>{plan.phase}</p>
      {plan.review !== null ? <p>{`Review ${plan.review.id}: ${plan.review.status}`}</p> : null}
      {plan.openEscalations > 0 ? <p>{`Escalations: ${plan.openEscalations}`}</p> : null}
      {plan.workflows.map((wf) => (
        <section key={wf.key} data-workflow={wf.key} aria-label={`Workflow ${wf.key}`}>
          <h3>{wf.key}</h3>
          <p>
            {wf.status} · {wf.repo} · {wf.base}
          </p>
          {wf.pr !== null ? (
            <p>
              <a href={wf.pr.url}>{`#${wf.pr.number} ${wf.pr.state ?? ""}`.trim()}</a>{" "}
              <button
                type="button"
                data-action="post-pr"
                onClick={() => onAction({ kind: "pr.post", planId: plan.planId, workflow: wf.key })}
              >
                Post PR
              </button>
            </p>
          ) : null}
          <ul>
            {wf.jobKeys.map((key) => {
              const job = plan.jobs.find((j) => j.key === key);
              if (job === undefined) throw new Error(`workflow ${wf.key} lists unknown job ${key}`);
              return <Job key={key} job={job} />;
            })}
          </ul>
        </section>
      ))}
      {explore.length > 0 ? (
        <section aria-label="Explore jobs">
          <ul>
            {explore.map((j) => (
              <Job key={j.key} job={j} />
            ))}
          </ul>
        </section>
      ) : null}
    </section>
  );
}
