import { createRouter, createWebHashHistory } from "vue-router";
import { nextTick } from "vue";
import { runViewTransition } from "../utils/motion";

const DashboardView = () => import("../views/DashboardView.vue");
const SettingsLayout = () => import("../layouts/SettingsLayout.vue");
const SessionsView = () => import("../views/SessionsView.vue");
const KeysView = () => import("../views/KeysView.vue");
const ScriptsView = () => import("../views/ScriptsView.vue");

const routes = [
  {
    path: "/",
    redirect: "/sessions",
  },
  {
    path: "/sessions",
    name: "sessions",
    component: SessionsView,
  },
  {
    path: "/workspace",
    name: "workspace",
    component: DashboardView,
  },
  {
    path: "/settings",
    name: "settings",
    component: SettingsLayout,
  },
  {
    path: "/keys",
    name: "keys",
    component: KeysView,
  },
  {
    path: "/scripts",
    name: "scripts",
    component: ScriptsView,
  },
  {
    path: "/credential-graph",
    redirect: "/keys",
  },
];

export const router = createRouter({
  history: createWebHashHistory(),
  routes,
});

const ROUTE_TRANSITION_CLASS = "route-transition-running";

router.beforeResolve((to, from) => {
  if (!from.name || to.name === from.name) {
    return true;
  }

  return new Promise((resolve) => {
    void runViewTransition(
      async () => {
        resolve(true);
        await nextTick();
      },
      { className: ROUTE_TRANSITION_CLASS },
    ).catch(() => {});
  });
});
