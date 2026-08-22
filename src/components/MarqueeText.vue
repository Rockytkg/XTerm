<script setup>
import "../styles/marquee-text.scss";

defineProps({
  text: {
    type: String,
    default: "",
  },
});

const MARQUEE_SPEED = 40; // px/s — 越低滚动越平缓

function onMouseEnter(event) {
  const label = event.currentTarget;
  const track = label.firstElementChild;
  const copy = track?.firstElementChild;
  if (!track || !copy) return;

  // 未溢出时无需滚动：copy 省略号裁剪，scrollWidth 仍是完整文本宽度
  if (copy.scrollWidth <= label.clientWidth) return;

  const duration = Math.max(2, (copy.scrollWidth + 24) / MARQUEE_SPEED);
  track.style.setProperty("--marquee-duration", `${duration}s`);
  track.classList.add("is-overflow");
}

function onMouseLeave(event) {
  const track = event.currentTarget.firstElementChild;
  if (!track) return;
  track.classList.remove("is-overflow");
  track.style.removeProperty("--marquee-duration");
}
</script>

<template>
  <span
    class="marquee-text"
    @mouseenter="onMouseEnter"
    @mouseleave="onMouseLeave"
  >
    <span class="marquee-text-track">
      <span class="marquee-text-copy">{{ text }}</span>
      <span
        class="marquee-text-copy"
        aria-hidden="true"
      >{{ text }}</span>
    </span>
  </span>
</template>
