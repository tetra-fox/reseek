import { DropdownMenu } from "bits-ui";

// root and trigger have no styles of their own, callers pass a Button through the trigger's child snippet
export const Root = DropdownMenu.Root;
export const Trigger = DropdownMenu.Trigger;
export { default as Content } from "./Content.svelte";
export const RadioGroup = DropdownMenu.RadioGroup;
export { default as Item } from "./Item.svelte";
export { default as RadioItem } from "./RadioItem.svelte";
export { default as Separator } from "./Separator.svelte";
export { default as Label } from "./Label.svelte";
