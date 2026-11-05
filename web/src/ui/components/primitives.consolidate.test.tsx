// @vitest-environment jsdom
import { expect, it } from "vitest";
import { mount } from "../../testing/render.tsx";
import { Alert, AlertDescription, AlertTitle } from "./alert.tsx";
import { Breadcrumb, BreadcrumbItem, BreadcrumbLink, BreadcrumbList, BreadcrumbPage, BreadcrumbSeparator } from "./breadcrumb.tsx";
import { CodeBlock, CodeBlockContent } from "./code-block.tsx";
import * as dropdown from "./dropdown-menu.tsx";
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from "./empty.tsx";
import * as hoverCard from "./hover-card.tsx";
import { InputGroup, InputGroupAddon, InputGroupTextarea } from "./input-group.tsx";
import { Message, MessageFooter, MessageHeader } from "./message.tsx";
import { Progress } from "./progress.tsx";
import * as select from "./select.tsx";
import { Spinner } from "./spinner.tsx";
import * as tooltip from "./tooltip.tsx";

function slot(root: HTMLElement, name: string): Element | null {
  return root.querySelector(`[data-slot="${name}"]`);
}

function expectFunctions(mod: Record<string, unknown>, names: string[]): void {
  for (const n of names) expect(typeof mod[n], `${n} export`).toBe("function");
}

it("alert primitive renders the destructive variant", () => {
  const m = mount(
    <Alert variant="destructive">
      <AlertTitle>Failed</AlertTitle>
      <AlertDescription>boom</AlertDescription>
    </Alert>,
  );
  const el = slot(m.container, "alert");
  expect(el, "alert element").not.toBeNull();
  expect(el?.getAttribute("role"), "role").toBe("alert");
  expect(el?.className, "destructive classes").toContain("text-destructive");
  expect(slot(m.container, "alert-title")?.textContent).toBe("Failed");
  expect(slot(m.container, "alert-description")?.textContent).toBe("boom");
  m.unmount();
});

it("breadcrumb primitive renders list, link, page and separator", () => {
  const m = mount(
    <Breadcrumb>
      <BreadcrumbList>
        <BreadcrumbItem>
          <BreadcrumbLink href="/host">host</BreadcrumbLink>
        </BreadcrumbItem>
        <BreadcrumbSeparator />
        <BreadcrumbItem>
          <BreadcrumbPage>lead</BreadcrumbPage>
        </BreadcrumbItem>
      </BreadcrumbList>
    </Breadcrumb>,
  );
  expect(slot(m.container, "breadcrumb")?.getAttribute("aria-label"), "nav label").toBe("breadcrumb");
  expect(slot(m.container, "breadcrumb-list")?.tagName, "list").toBe("OL");
  expect(m.container.querySelectorAll('[data-slot="breadcrumb-item"]').length, "items").toBe(2);
  expect(slot(m.container, "breadcrumb-link")?.getAttribute("href"), "link href").toBe("/host");
  expect(slot(m.container, "breadcrumb-page")?.getAttribute("aria-current"), "page current").toBe("page");
  expect(slot(m.container, "breadcrumb-separator"), "separator").not.toBeNull();
  m.unmount();
});

it("code-block primitive renders its source text", () => {
  const m = mount(
    <CodeBlock>
      <CodeBlockContent>{"const a = 1;"}</CodeBlockContent>
    </CodeBlock>,
  );
  expect(slot(m.container, "code-block"), "root").not.toBeNull();
  expect(slot(m.container, "code-block-content")?.tagName, "pre").toBe("PRE");
  expect(slot(m.container, "code-block-code")?.textContent, "source").toBe("const a = 1;");
  m.unmount();
});

it("dropdown-menu primitive exports its parts", () => {
  expectFunctions(dropdown, [
    "DropdownMenu", "DropdownMenuPortal", "DropdownMenuTrigger", "DropdownMenuContent", "DropdownMenuGroup",
    "DropdownMenuItem", "DropdownMenuCheckboxItem", "DropdownMenuRadioGroup", "DropdownMenuRadioItem",
    "DropdownMenuLabel", "DropdownMenuSeparator", "DropdownMenuShortcut", "DropdownMenuSub",
    "DropdownMenuSubTrigger", "DropdownMenuSubContent",
  ]);
  const m = mount(
    <dropdown.DropdownMenu>
      <dropdown.DropdownMenuTrigger>open</dropdown.DropdownMenuTrigger>
    </dropdown.DropdownMenu>,
  );
  expect(slot(m.container, "dropdown-menu-trigger")?.textContent, "trigger").toBe("open");
  m.unmount();
});

it("empty primitive renders title and description", () => {
  const m = mount(
    <Empty>
      <EmptyHeader>
        <EmptyTitle>Nothing</EmptyTitle>
        <EmptyDescription>Add one</EmptyDescription>
      </EmptyHeader>
    </Empty>,
  );
  expect(slot(m.container, "empty"), "root").not.toBeNull();
  expect(slot(m.container, "empty-title")?.textContent).toBe("Nothing");
  expect(slot(m.container, "empty-description")?.textContent).toBe("Add one");
  m.unmount();
});

it("hover-card primitive exports its parts", () => {
  expectFunctions(hoverCard, ["HoverCard", "HoverCardTrigger", "HoverCardContent"]);
  const m = mount(
    <hoverCard.HoverCard>
      <hoverCard.HoverCardTrigger>ctx</hoverCard.HoverCardTrigger>
    </hoverCard.HoverCard>,
  );
  expect(slot(m.container, "hover-card-trigger")?.textContent, "trigger").toBe("ctx");
  m.unmount();
});

it("input-group primitive renders addon and textarea", () => {
  const m = mount(
    <InputGroup>
      <InputGroupTextarea placeholder="Say" />
      <InputGroupAddon align="block-end">tools</InputGroupAddon>
    </InputGroup>,
  );
  const root = slot(m.container, "input-group");
  expect(root, "root").not.toBeNull();
  expect(root?.getAttribute("role"), "group role").toBe("group");
  expect(slot(m.container, "input-group-control")?.tagName, "textarea control").toBe("TEXTAREA");
  expect(slot(m.container, "input-group-addon")?.textContent, "addon").toBe("tools");
  m.unmount();
});

it("message primitive renders header, bubble and footer", () => {
  const m = mount(
    <Message>
      <MessageHeader>me</MessageHeader>
      <div>body</div>
      <MessageFooter>12:00</MessageFooter>
    </Message>,
  );
  expect(slot(m.container, "message"), "root").not.toBeNull();
  expect(slot(m.container, "message-header")?.textContent, "header").toBe("me");
  expect(slot(m.container, "message-footer")?.textContent, "footer").toBe("12:00");
  m.unmount();
});

it("progress primitive renders its value", () => {
  const m = mount(<Progress value={40} />);
  const el = slot(m.container, "progress");
  expect(el, "root").not.toBeNull();
  expect(el?.getAttribute("role"), "role").toBe("progressbar");
  expect(el?.getAttribute("aria-valuenow"), "value").toBe("40");
  expect(slot(m.container, "progress-indicator"), "indicator").not.toBeNull();
  m.unmount();
});

it("select primitive exports its parts", () => {
  expectFunctions(select, [
    "Select", "SelectGroup", "SelectValue", "SelectTrigger", "SelectContent", "SelectLabel", "SelectItem",
    "SelectSeparator", "SelectScrollUpButton", "SelectScrollDownButton",
  ]);
  const m = mount(
    <select.Select defaultValue="a">
      <select.SelectTrigger>
        <select.SelectValue />
      </select.SelectTrigger>
    </select.Select>,
  );
  expect(slot(m.container, "select-trigger"), "trigger").not.toBeNull();
  m.unmount();
});

it("spinner primitive renders an svg", () => {
  const m = mount(<Spinner className="extra" />);
  const el = slot(m.container, "spinner");
  expect(el?.tagName.toLowerCase(), "svg").toBe("svg");
  expect(el?.getAttribute("role"), "role").toBe("status");
  expect(el?.getAttribute("aria-label"), "label").toBe("Loading");
  expect(el?.getAttribute("class"), "className merged").toContain("extra");
  m.unmount();
});

it("tooltip primitive exports its parts", () => {
  expectFunctions(tooltip, ["TooltipProvider", "Tooltip", "TooltipTrigger", "TooltipContent"]);
  const m = mount(
    <tooltip.TooltipProvider>
      <tooltip.Tooltip>
        <tooltip.TooltipTrigger>copy</tooltip.TooltipTrigger>
      </tooltip.Tooltip>
    </tooltip.TooltipProvider>,
  );
  expect(slot(m.container, "tooltip-trigger")?.textContent, "trigger").toBe("copy");
  m.unmount();
});
