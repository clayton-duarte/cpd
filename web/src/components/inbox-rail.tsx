// The Inbox rail: sidebar-09's mail list, vendored 1:1 from the `new-york-v4` registry into the
// shell's right column (web/README.md "The Inbox rail"). Three parts of the block are
// deliberately not vendored: its first sidebar, the icon column (Inbox/Drafts/Sent/Junk/Trash,
// Acme Inc, the sample user), the `Unreads` switch that sat in this header, and the
// `Type to search...` input the block puts under the title. What is vendored is the block's
// second sidebar: its header (the title) and its mail list, rows and classes verbatim. The mails
// are the registry's own sample data, verbatim; reading the real Inbox is a follow-up.
import type { ReactElement } from "react";
import {
  Sidebar,
  SidebarContent,
  SidebarGroup,
  SidebarGroupContent,
  SidebarHeader,
} from "./ui/sidebar";

/** One mail, the five fields the block's sample data declares. */
interface Mail {
  name: string;
  email: string;
  subject: string;
  date: string;
  teaser: string;
}

/** The registry's sample data, verbatim (sidebar-09's `data.mails`, in its order). */
const mails: readonly Mail[] = [
  {
    name: "William Smith",
    email: "williamsmith@example.com",
    subject: "Meeting Tomorrow",
    date: "09:34 AM",
    teaser:
      "Hi team, just a reminder about our meeting tomorrow at 10 AM.\nPlease come prepared with your project updates.",
  },
  {
    name: "Alice Smith",
    email: "alicesmith@example.com",
    subject: "Re: Project Update",
    date: "Yesterday",
    teaser:
      "Thanks for the update. The progress looks great so far.\nLet's schedule a call to discuss the next steps.",
  },
  {
    name: "Bob Johnson",
    email: "bobjohnson@example.com",
    subject: "Weekend Plans",
    date: "2 days ago",
    teaser:
      "Hey everyone! I'm thinking of organizing a team outing this weekend.\nWould you be interested in a hiking trip or a beach day?",
  },
  {
    name: "Emily Davis",
    email: "emilydavis@example.com",
    subject: "Re: Question about Budget",
    date: "2 days ago",
    teaser:
      "I've reviewed the budget numbers you sent over.\nCan we set up a quick call to discuss some potential adjustments?",
  },
  {
    name: "Michael Wilson",
    email: "michaelwilson@example.com",
    subject: "Important Announcement",
    date: "1 week ago",
    teaser:
      "Please join us for an all-hands meeting this Friday at 3 PM.\nWe have some exciting news to share about the company's future.",
  },
  {
    name: "Sarah Brown",
    email: "sarahbrown@example.com",
    subject: "Re: Feedback on Proposal",
    date: "1 week ago",
    teaser:
      "Thank you for sending over the proposal. I've reviewed it and have some thoughts.\nCould we schedule a meeting to discuss my feedback in detail?",
  },
  {
    name: "David Lee",
    email: "davidlee@example.com",
    subject: "New Project Idea",
    date: "1 week ago",
    teaser:
      "I've been brainstorming and came up with an interesting project concept.\nDo you have time this week to discuss its potential impact and feasibility?",
  },
  {
    name: "Olivia Wilson",
    email: "oliviawilson@example.com",
    subject: "Vacation Plans",
    date: "1 week ago",
    teaser:
      "Just a heads up that I'll be taking a two-week vacation next month.\nI'll make sure all my projects are up to date before I leave.",
  },
  {
    name: "James Martin",
    email: "jamesmartin@example.com",
    subject: "Re: Conference Registration",
    date: "1 week ago",
    teaser:
      "I've completed the registration for the upcoming tech conference.\nLet me know if you need any additional information from my end.",
  },
  {
    name: "Sophia White",
    email: "sophiawhite@example.com",
    subject: "Team Dinner",
    date: "1 week ago",
    teaser:
      "To celebrate our recent project success, I'd like to organize a team dinner.\nAre you available next Friday evening? Please let me know your preferences.",
  },
];

/**
 * The Inbox rail: the block's mail list, one row per mail — the name with its date pushed right,
 * the subject, then the two-line teaser — under its header's title. The rows are anchors that go
 * nowhere yet, as the block renders them; the block's search input is not vendored.
 */
export function InboxRail(): ReactElement {
  return (
    <Sidebar collapsible="none">
      <SidebarHeader className="gap-3.5 border-b p-4">
        <div className="text-base font-medium text-foreground">Inbox</div>
      </SidebarHeader>
      <SidebarContent>
        <SidebarGroup className="px-0">
          <SidebarGroupContent>
            {mails.map((mail) => (
              <a
                href="#"
                key={mail.email}
                className="flex flex-col items-start gap-2 border-b p-4 text-sm leading-tight whitespace-nowrap last:border-b-0 hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
              >
                <div className="flex w-full items-center gap-2">
                  <span>{mail.name}</span>{" "}
                  <span className="ml-auto text-xs">{mail.date}</span>
                </div>
                <span className="font-medium">{mail.subject}</span>
                <span className="line-clamp-2 w-[260px] text-xs whitespace-break-spaces">
                  {mail.teaser}
                </span>
              </a>
            ))}
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>
    </Sidebar>
  );
}
