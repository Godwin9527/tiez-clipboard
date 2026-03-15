import { useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type HotkeyMode = "main" | "sequential" | "rich" | "search" | "scroll_top" | "emoji_panel";

interface UseHotkeyConfigOptions {
  hotkey: string;
  setHotkey: (val: string) => void;
  sequentialHotkey: string;
  setSequentialHotkey: (val: string) => void;
  richPasteHotkey: string;
  setRichPasteHotkey: (val: string) => void;
  searchHotkey: string;
  setSearchHotkey: (val: string) => void;
  scrollTopHotkey: string;
  setScrollTopHotkey: (val: string) => void;
  emojiPanelHotkey: string;
  setEmojiPanelHotkey: (val: string) => void;
  sequentialMode: boolean;
  isRecording: boolean;
  setIsRecording: (val: boolean) => void;
  isRecordingSequential: boolean;
  setIsRecordingSequential: (val: boolean) => void;
  isRecordingRich: boolean;
  setIsRecordingRich: (val: boolean) => void;
  isRecordingSearch: boolean;
  setIsRecordingSearch: (val: boolean) => void;
  isRecordingScrollTop: boolean;
  setIsRecordingScrollTop: (val: boolean) => void;
  isRecordingEmojiPanel: boolean;
  setIsRecordingEmojiPanel: (val: boolean) => void;
  saveAppSetting: (type: string, value: string) => void;
  t: (key: string) => string;
  pushToast: (msg: string, duration?: number) => number;
}

export const useHotkeyConfig = ({
  hotkey, setHotkey,
  sequentialHotkey, setSequentialHotkey,
  richPasteHotkey, setRichPasteHotkey,
  searchHotkey, setSearchHotkey,
  scrollTopHotkey, setScrollTopHotkey,
  emojiPanelHotkey, setEmojiPanelHotkey,
  sequentialMode,
  isRecording, setIsRecording,
  isRecordingSequential, setIsRecordingSequential,
  isRecordingRich, setIsRecordingRich,
  isRecordingSearch, setIsRecordingSearch,
  isRecordingScrollTop, setIsRecordingScrollTop,
  isRecordingEmojiPanel, setIsRecordingEmojiPanel,
  saveAppSetting, t, pushToast
}: UseHotkeyConfigOptions) => {
  const allHotkeys = [
    { mode: "main" as HotkeyMode, key: hotkey, label: "global_hotkey" },
    { mode: "sequential" as HotkeyMode, key: sequentialHotkey, label: "sequential_paste_hotkey_label", enabled: sequentialMode },
    { mode: "rich" as HotkeyMode, key: richPasteHotkey, label: "rich_paste_hotkey_label" },
    { mode: "search" as HotkeyMode, key: searchHotkey, label: "search_hotkey_label" },
    { mode: "scroll_top" as HotkeyMode, key: scrollTopHotkey, label: "scroll_top_hotkey_label" },
    { mode: "emoji_panel" as HotkeyMode, key: emojiPanelHotkey, label: "emoji_panel_hotkey_label" },
  ];

  const checkHotkeyConflict = useCallback(
    (newHotkey: string, mode: HotkeyMode): boolean => {
      if (!newHotkey) return false;
      const conflicts = [];
      for (const h of allHotkeys) {
        if (h.mode !== mode && newHotkey === h.key && (h.enabled !== false)) {
          conflicts.push(t(h.label));
        }
      }
      if (conflicts.length > 0) {
        const msg = t("hotkey_conflict_toast").replace("{name}", conflicts[0]);
        pushToast(msg, 5000);
        return true;
      }
      return false;
    },
    [hotkey, sequentialMode, sequentialHotkey, richPasteHotkey, searchHotkey, scrollTopHotkey, emojiPanelHotkey, t, pushToast]
  );

  const makeUpdater = (
    mode: HotkeyMode,
    setter: (v: string) => void,
    setRecording: (v: boolean) => void,
    settingKey: string,
    commandName: string
  ) => useCallback(
    async (newHotkey: string) => {
      if (checkHotkeyConflict(newHotkey, mode)) { setRecording(false); return; }
      if (newHotkey) {
        try { await invoke<boolean>("test_hotkey_available", { hotkey: newHotkey }); }
        catch (err) { pushToast(`❌ ${newHotkey}: ${err || "快捷键被占用"}`, 5000); setRecording(false); return; }
      }
      setter(newHotkey);
      saveAppSetting(settingKey, newHotkey);
      await invoke(commandName, { hotkey: newHotkey }).catch(console.error);
      setRecording(false);
    },
    [checkHotkeyConflict, pushToast, saveAppSetting, setter, setRecording]
  );

  const updateHotkey = useCallback(
    async (newHotkey: string) => {
      if (checkHotkeyConflict(newHotkey, "main")) { setIsRecording(false); return; }
      if (newHotkey) {
        try { await invoke<boolean>("test_hotkey_available", { hotkey: newHotkey }); }
        catch (err) { pushToast(`❌ ${newHotkey}: ${err || "快捷键被占用"}`, 5000); setIsRecording(false); return; }
      }
      setHotkey(newHotkey);
      saveAppSetting("hotkey", newHotkey);
      await invoke("register_hotkey", { hotkey: newHotkey }).catch((err) => {
        if (newHotkey) pushToast(t("hotkey_register_failed") + (err?.toString() || ""), 3000);
      });
      setIsRecording(false);
    },
    [checkHotkeyConflict, pushToast, saveAppSetting, setHotkey, setIsRecording, t]
  );

  const updateSequentialHotkey = makeUpdater("sequential", setSequentialHotkey, setIsRecordingSequential, "sequential_hotkey", "set_sequential_hotkey");
  const updateRichPasteHotkey = makeUpdater("rich", setRichPasteHotkey, setIsRecordingRich, "rich_paste_hotkey", "set_rich_paste_hotkey");
  const updateSearchHotkey = makeUpdater("search", setSearchHotkey, setIsRecordingSearch, "search_hotkey", "set_search_hotkey");
  const updateScrollTopHotkey = makeUpdater("scroll_top", setScrollTopHotkey, setIsRecordingScrollTop, "scroll_top_hotkey", "set_scroll_top_hotkey");
  const updateEmojiPanelHotkey = makeUpdater("emoji_panel", setEmojiPanelHotkey, setIsRecordingEmojiPanel, "emoji_panel_hotkey", "set_emoji_panel_hotkey");

  const anyRecording = isRecording || isRecordingSequential || isRecordingRich || isRecordingSearch || isRecordingScrollTop || isRecordingEmojiPanel;

  useEffect(() => {
    invoke("set_recording_mode", { enabled: anyRecording }).catch(console.error);

    if (anyRecording) {
      const unlisten = listen<string>("hotkey-recorded", (event) => {
        if (isRecording) updateHotkey(event.payload);
        if (isRecordingSequential) updateSequentialHotkey(event.payload);
        if (isRecordingRich) updateRichPasteHotkey(event.payload);
        if (isRecordingSearch) updateSearchHotkey(event.payload);
        if (isRecordingScrollTop) updateScrollTopHotkey(event.payload);
        if (isRecordingEmojiPanel) updateEmojiPanelHotkey(event.payload);
      });

      const unlistenCancel = listen("recording-cancelled", () => {
        setIsRecording(false);
        setIsRecordingSequential(false);
        setIsRecordingRich(false);
        setIsRecordingSearch(false);
        setIsRecordingScrollTop(false);
        setIsRecordingEmojiPanel(false);
      });

      return () => {
        unlisten.then((f) => f());
        unlistenCancel.then((f) => f());
      };
    }
  }, [
    anyRecording,
    isRecording, isRecordingSequential, isRecordingRich, isRecordingSearch, isRecordingScrollTop, isRecordingEmojiPanel,
    setIsRecording, setIsRecordingSequential, setIsRecordingRich, setIsRecordingSearch, setIsRecordingScrollTop, setIsRecordingEmojiPanel,
    updateHotkey, updateSequentialHotkey, updateRichPasteHotkey, updateSearchHotkey, updateScrollTopHotkey, updateEmojiPanelHotkey
  ]);

  return {
    checkHotkeyConflict,
    updateHotkey, updateSequentialHotkey, updateRichPasteHotkey, updateSearchHotkey,
    updateScrollTopHotkey, updateEmojiPanelHotkey
  };
};
