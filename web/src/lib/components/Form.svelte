<script lang="ts" module>
  import { createContext } from "svelte";

  type Problem = { id: string; name: string; message: string };

  export const [problems, setProblems] = createContext<() => Problem[]>();
</script>

<script lang="ts">
  import type { HTMLFormAttributes } from "svelte/elements";

  type Props = HTMLFormAttributes & {
    // row: one line under the form naming the first invalid field. stacked: FieldError under each field
    layout: "row" | "stacked";
  };

  let { layout, onsubmit, oninput, children, ...rest }: Props = $props();

  let list = $state<Problem[]>([]);
  // a row form's one line already names the problem, so its fields' FieldErrors stay empty
  setProblems(() => (layout === "stacked" ? list : []));

  type Field = HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement;

  const nameOf = (el: Field) =>
    el.getAttribute("aria-label") ??
    el.labels?.[0]?.textContent?.trim() ??
    el.getAttribute("placeholder") ??
    el.name;

  // the browser still decides what's valid through constraint validation, novalidate only drops its bubble
  const check = (form: HTMLFormElement) => {
    const fields = [...form.elements].filter(
      (el): el is Field => "willValidate" in el && (el as Field).willValidate
    );
    list = fields.flatMap((el) => {
      const ok = el.checkValidity();
      if (ok) el.removeAttribute("aria-invalid");
      else el.setAttribute("aria-invalid", "true");
      return ok ? [] : [{ id: el.id, name: nameOf(el), message: el.validationMessage }];
    });
  };
</script>

<form
  novalidate
  onsubmit={(e) => {
    check(e.currentTarget);
    if (list.length) e.preventDefault();
    else onsubmit?.(e);
  }}
  oninput={(e) => {
    if (list.length) check(e.currentTarget);
    oninput?.(e);
  }}
  {...rest}>
  {@render children?.()}
  {#if layout === "row" && list[0]}
    <p class="basis-full text-xs text-danger" role="alert">{list[0].name}: {list[0].message}</p>
  {/if}
</form>
