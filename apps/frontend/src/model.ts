export type PageName = "Overview" | "Inventory" | "Review Queue" | "Health" | "Dependencies" | "Activity" | "Adapter Packs" | "Backups" | "Settings";

export type Discovery = {
  id: string;
  suggested_name: string;
  suggested_type: string;
  source_scanner: string;
  confidence: "low" | "medium" | "high";
  evidence: { kind: string; summary: string }[];
  observed_at: string;
  health_state: string;
};

export type BootstrapState = {
  mode: "desktop" | "demo";
  pending: Discovery[];
  inventory: Discovery[];
};
