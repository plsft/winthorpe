import {
	act,
	cleanup,
	fireEvent,
	render,
	screen,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { ContextBar } from "./index";

beforeEach(() => {
	vi.useFakeTimers();
});

afterEach(() => {
	vi.useRealTimers();
	cleanup();
});

describe("ContextBar", () => {
	test("renders nothing when there are no directories", () => {
		const { container } = render(
			<ContextBar directories={[]} onRemove={() => {}} />,
		);
		expect(container.firstChild).toBeNull();
	});

	test("renders one chip per directory with the display name, hides branch + path", () => {
		render(
			<ContextBar
				directories={[
					{ path: "/code/sdk", name: "winthorpe-sdk", branch: "main" },
					{
						path: "/code/sidecar",
						name: "winthorpe-sidecar",
						branch: "feat/cli",
					},
				]}
				onRemove={() => {}}
			/>,
		);
		expect(screen.getByText("winthorpe-sdk")).toBeInTheDocument();
		expect(screen.getByText("winthorpe-sidecar")).toBeInTheDocument();
		// Branch labels and full paths are intentionally NOT shown on chips —
		// the chip is just "this workspace is linked" and the branch/path
		// come from the popup / hover tooltip respectively.
		expect(screen.queryByText("main")).not.toBeInTheDocument();
		expect(screen.queryByText("feat/cli")).not.toBeInTheDocument();
		expect(screen.queryByText("/code/sdk")).not.toBeInTheDocument();
	});

	test("derives display name from basename when name is missing", () => {
		render(
			<ContextBar
				directories={[{ path: "/a/b/charlie", branch: null }]}
				onRemove={() => {}}
			/>,
		);
		expect(screen.getByText("charlie")).toBeInTheDocument();
	});

	test("shows CONTEXT label above the chip list", () => {
		render(
			<ContextBar
				directories={[{ path: "/p", branch: null }]}
				onRemove={() => {}}
			/>,
		);
		expect(screen.getByText("context")).toBeInTheDocument();
	});

	test("hovering a chip pops a tooltip with the full path after a short delay", () => {
		render(
			<ContextBar
				directories={[
					{ path: "/Users/me/longpath", name: "longpath", branch: null },
				]}
				onRemove={() => {}}
			/>,
		);
		const chip = screen.getByText("longpath").closest("[data-chip]");
		expect(chip).toBeInTheDocument();
		fireEvent.mouseOver(chip as HTMLElement);
		// Tooltip doesn't appear immediately — it's delayed by 350ms.
		expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
		act(() => {
			vi.advanceTimersByTime(400);
		});
		const tooltip = screen.getByRole("tooltip");
		expect(tooltip).toHaveTextContent("/Users/me/longpath");
	});

	test("clicking the × button on a chip fires onRemove immediately", () => {
		const onRemove = vi.fn();
		render(
			<ContextBar
				directories={[{ path: "/a", name: "a", branch: null }]}
				onRemove={onRemove}
			/>,
		);
		fireEvent.click(screen.getByLabelText("Remove a"));
		// No collapse animation — removal is synchronous.
		expect(onRemove).toHaveBeenCalledWith("/a");
	});

	test("Backspace on a focused chip removes it and preserves keyboard ordering", () => {
		const onRemove = vi.fn();
		render(
			<ContextBar
				directories={[
					{ path: "/a", name: "a", branch: null },
					{ path: "/b", name: "b", branch: null },
				]}
				onRemove={onRemove}
			/>,
		);
		const chipA = screen.getByText("a").closest("[data-chip]") as HTMLElement;
		chipA.focus();
		fireEvent.keyDown(chipA, { key: "Backspace" });
		expect(onRemove).toHaveBeenCalledWith("/a");
	});

	test("ArrowRight moves focus to the next chip", () => {
		render(
			<ContextBar
				directories={[
					{ path: "/a", name: "a", branch: null },
					{ path: "/b", name: "b", branch: null },
				]}
				onRemove={() => {}}
			/>,
		);
		const chipA = screen.getByText("a").closest("[data-chip]") as HTMLElement;
		const chipB = screen.getByText("b").closest("[data-chip]") as HTMLElement;
		chipA.focus();
		fireEvent.keyDown(chipA, { key: "ArrowRight" });
		expect(document.activeElement).toBe(chipB);
	});

	test("Home jumps focus to the first chip, End to the last", () => {
		render(
			<ContextBar
				directories={[
					{ path: "/a", name: "a", branch: null },
					{ path: "/b", name: "b", branch: null },
					{ path: "/c", name: "c", branch: null },
				]}
				onRemove={() => {}}
			/>,
		);
		const chipB = screen.getByText("b").closest("[data-chip]") as HTMLElement;
		chipB.focus();
		fireEvent.keyDown(chipB, { key: "Home" });
		expect(document.activeElement).toBe(
			screen.getByText("a").closest("[data-chip]"),
		);
		fireEvent.keyDown(document.activeElement as HTMLElement, { key: "End" });
		expect(document.activeElement).toBe(
			screen.getByText("c").closest("[data-chip]"),
		);
	});

	test("Escape blurs the focused chip", () => {
		render(
			<ContextBar
				directories={[{ path: "/a", name: "a", branch: null }]}
				onRemove={() => {}}
			/>,
		);
		const chip = screen.getByText("a").closest("[data-chip]") as HTMLElement;
		chip.focus();
		expect(document.activeElement).toBe(chip);
		fireEvent.keyDown(chip, { key: "Escape" });
		expect(document.activeElement).not.toBe(chip);
	});
});
