export type TabId = "disk" | "cleanup" | "processes" | "caches" | "apps" | "recommendations" | "health" | "drives";

/** Current sidebar tab, shared so any view can link to another. */
export const nav = $state<{ tab: TabId }>({ tab: "disk" });
