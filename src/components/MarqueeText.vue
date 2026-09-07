<script>
// 悬停跑马灯共享实现：label 容器内的 track 仅在内容溢出（width 超过
// label 可视宽度）时滚动，经 is-overflow 类与 --marquee-duration 变量驱动动画。
export function startHoverMarquee(label, { width, speed, minDuration, gap }) {
  const track = label?.firstElementChild;
  if (!track || !Number.isFinite(width) || width <= label.clientWidth) return;
  const duration = Math.max(minDuration, (width + gap) / speed);
  track.style.setProperty("--marquee-duration", `${duration}s`);
  track.classList.add("is-overflow");
}

export function stopHoverMarquee(label) {
  const track = label?.firstElementChild;
  if (!track) return;
  track.classList.remove("is-overflow");
  track.style.removeProperty("--marquee-duration");
}
</script>

<script setup>
import "../styles/marquee-text.scss";

const props = defineProps({
  text: {
    type: String,
    default: "",
  },
  // px/s — 越低滚动越平缓
  speed: {
    type: Number,
    default: 40,
  },
});

function onMouseEnter(event) {
  const label = event.currentTarget;
  const copy = label.firstElementChild?.firstElementChild;
  // 未溢出时无需滚动：copy 省略号裁剪，scrollWidth 仍是完整文本宽度
  startHoverMarquee(label, {
    width: copy?.scrollWidth ?? 0,
    speed: props.speed,
    minDuration: 2,
    gap: 24,
  });
}

function onMouseLeave(event) {
  stopHoverMarquee(event.currentTarget);
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
