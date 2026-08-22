/* global xterm */ // injected by the scripting engine sandbox

// ==XTermScript==
// @name        交换机开局配置
// @author      xterm
// @description 主机名 + 管理 VLAN + 保存配置（docs/SCRIPTING.md 完整示例）
// @version     1.1.0
// ==/XTermScript==

const values = await xterm.form({
  title: "开局配置",
  fields: [
    { key: "hostname", label: "主机名", defaultValue: "SW1", required: true },
    {
      key: "vlanId",
      label: "管理 VLAN",
      type: "number",
      required: true,
      pattern: /^\d{1,4}$/,
      message: "VLAN 必须是 1-4094 的数字",
    },
    { key: "save", label: "完成后保存配置", type: "switch", defaultValue: true },
  ],
});

await xterm.sendLine("system-view");
await xterm.waitFor("]", 5000, "未进入系统视图");

await xterm.sendLine(`sysname ${values.hostname}`);
await xterm.waitFor(`[${values.hostname}]`, 5000);

await xterm.sendLine(`vlan ${values.vlanId}`);
await xterm.waitFor(`[${values.hostname}-vlan${values.vlanId}]`, 5000);
await xterm.sendLine("quit");

if (values.save) {
  await xterm.sendLine("save");
  await xterm.waitForAny(["Y/N", "yes/no"], 5000);
  await xterm.sendLine("y");
  await xterm.waitFor("successfully", 8000, "保存配置失败");
}

await xterm.alert(`${values.hostname} 开局完成`);
xterm.log("开局配置完成", values);
