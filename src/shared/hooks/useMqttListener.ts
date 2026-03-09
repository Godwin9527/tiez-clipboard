import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";

export const useMqttListener = () => {
  useEffect(() => {
    const unlistenMqtt = listen<string>("mqtt-message", async (event) => {
      // The backend already handles writing to the system clipboard via arboard.
      // We listen to the event here in case we want to show a notification or update UI,
      // but writing to navigator.clipboard is redundant and can cause issues.
      void event.payload;
    });

    return () => {
      unlistenMqtt.then((f) => f());
    };
  }, []);
};
