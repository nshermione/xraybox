export type SettingKind = "toggle" | "value" | "link";

export interface SettingItem {
  id: string;
  kind: SettingKind;
  value?: string;
  enabled?: boolean;
  iconColor: string;
  screen?: "list" | "fields" | "vless" | "dns";
  labelKey?: string;
}

export interface SettingGroup {
  id: string;
  items: SettingItem[];
}

export const settingGroups: SettingGroup[] = [
  {
    id: "general",
    items: [
      {
        id: "auto-connect",
        kind: "toggle",
        enabled: true,
        iconColor: "hsl(210 70% 48%)",
      },
      {
        id: "start-boot",
        kind: "toggle",
        enabled: false,
        iconColor: "hsl(230 70% 48%)",
      },
      {
        id: "kill-switch",
        kind: "toggle",
        enabled: false,
        iconColor: "hsl(200 60% 44%)",
      },
      {
        id: "appearance",
        kind: "value",
        iconColor: "hsl(250 60% 50%)",
        screen: "list",
      },
      {
        id: "language",
        kind: "value",
        iconColor: "hsl(175 50% 40%)",
        screen: "list",
      },
      {
        id: "smart-connect",
        kind: "toggle",
        enabled: false,
        iconColor: "hsl(145 55% 40%)",
      },
      {
        id: "fetch-interval",
        kind: "value",
        iconColor: "hsl(30 75% 48%)",
        screen: "list",
      },
    ],
  },
  {
    id: "network",
    items: [
      {
        id: "manual-endpoint",
        kind: "link",
        iconColor: "hsl(200 70% 44%)",
        screen: "vless",
      },
      {
        id: "rules",
        kind: "value",
        value: "1",
        iconColor: "hsl(20 75% 48%)",
        screen: "list",
      },
      {
        id: "remote-dns",
        kind: "value",
        value: "1.1.1.1, 8.8.8.8",
        iconColor: "hsl(280 55% 50%)",
        screen: "dns",
      },
    ],
  },
  {
    id: "diagnostics",
    items: [
      {
        id: "logs",
        kind: "link",
        iconColor: "hsl(0 0% 40%)",
        screen: "list",
      },
    ],
  },
];

export function nestedItemsFor(parentId: string): SettingItem[] {
  return [
    {
      id: `${parentId}-enabled`,
      labelKey: "nested.enabled",
      kind: "toggle",
      enabled: true,
      iconColor: "hsl(145 60% 40%)",
    },
    {
      id: `${parentId}-mode`,
      labelKey: "nested.mode",
      kind: "value",
      value: "Auto",
      iconColor: "hsl(210 70% 48%)",
      screen: "list",
    },
    {
      id: `${parentId}-advanced`,
      labelKey: "nested.advanced",
      kind: "link",
      iconColor: "hsl(230 60% 48%)",
      screen: "list",
    },
  ];
}
