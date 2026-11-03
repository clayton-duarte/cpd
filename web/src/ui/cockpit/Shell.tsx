import type { ReactElement, ReactNode } from "react";

export function Shell(props: { tree: ReactNode; plan: ReactNode; chat: ReactNode; inbox: ReactNode; header?: ReactNode; status?: ReactNode }): ReactElement {
  return (
    <div data-shell>
      {props.header === undefined ? null : <header aria-label="Cockpit header">{props.header}</header>}
      <aside aria-label="Context tree column">{props.tree}</aside>
      <div data-column="center">
        <section aria-label="Plan pane">{props.plan}</section>
        <main aria-label="Chat">{props.chat}</main>
      </div>
      <aside aria-label="Inbox column">{props.inbox}</aside>
      {props.status === undefined ? null : <footer aria-label="Status bar">{props.status}</footer>}
    </div>
  );
}
