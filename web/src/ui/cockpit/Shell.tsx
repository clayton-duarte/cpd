import type { ReactElement, ReactNode } from "react";

export function Shell(props: { tree: ReactNode; plan: ReactNode; chat: ReactNode; inbox: ReactNode }): ReactElement {
  return (
    <div data-shell>
      <aside aria-label="Context tree column">{props.tree}</aside>
      <div data-column="center">
        <section aria-label="Plan pane">{props.plan}</section>
        <main aria-label="Chat">{props.chat}</main>
      </div>
      <aside aria-label="Inbox column">{props.inbox}</aside>
    </div>
  );
}
