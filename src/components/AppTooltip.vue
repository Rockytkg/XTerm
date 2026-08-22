<script setup>
import { TooltipContent, TooltipPortal, TooltipRoot, TooltipTrigger } from "reka-ui";
import "../styles/tooltip.scss";

defineProps({
  content: {
    type: String,
    required: true,
  },
  side: {
    type: String,
    default: "right",
  },
  disabled: {
    type: Boolean,
    default: false,
  },
  sideOffset: {
    type: Number,
    default: 10,
  },
});
</script>

<template>
  <TooltipRoot :disabled="disabled">
    <TooltipTrigger as-child>
      <slot />
    </TooltipTrigger>
    <TooltipPortal>
      <TooltipContent
        :side="side"
        :side-offset="sideOffset"
        class="tooltip-content z-50 rounded-[12px] border border-border bg-bg-secondary px-[12px] py-[8px] text-[0.8571em] text-text-primary shadow-md"
      >
        {{ content }}
      </TooltipContent>
    </TooltipPortal>
  </TooltipRoot>
</template>
