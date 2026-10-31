<script lang="ts">
  import { goto } from "$app/navigation";

  import * as Menu from "#lib/components/ui/menu/menu.js";
  import { reach, type UserActionId } from "#lib/users/actions.js";

  type Props = {
    name: string;
    /** the folder the name was shown with, which the user-scoped search starts from */
    folder?: string;
    /** the ways to list, every way when not given */
    only?: UserActionId[];
    /** the items name the user, for a menu that is about something else */
    named?: boolean;
  };

  let { name, folder, only, named = false }: Props = $props();
</script>

{#each reach(name, { only, folder }) as action (action.id)}
  <Menu.Item onSelect={() => goto(action.href)}
    ><action.icon />{named ? action.of(name) : action.label}</Menu.Item>
{/each}
