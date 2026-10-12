const STORAGE_KEYS = {
  API_BASE_URL: "lumen_pod_api_base_url",
  API_TOKEN: "lumen_pod_api_token",
} as const;

const DEFAULT_API_BASE_URL = "https://lumenpod.lumenopen.com";

export async function getApiConfig(): Promise<{
  baseUrl: string;
  token: string;
}> {
  const result = await chrome.storage.local.get([
    STORAGE_KEYS.API_BASE_URL,
    STORAGE_KEYS.API_TOKEN,
  ]);
  return {
    baseUrl:
      (result[STORAGE_KEYS.API_BASE_URL] as string) || DEFAULT_API_BASE_URL,
    token: (result[STORAGE_KEYS.API_TOKEN] as string) || "",
  };
}

export async function setApiConfig(baseUrl: string, token: string) {
  await chrome.storage.local.set({
    [STORAGE_KEYS.API_BASE_URL]: baseUrl,
    [STORAGE_KEYS.API_TOKEN]: token,
  });
}

export { STORAGE_KEYS, DEFAULT_API_BASE_URL };
