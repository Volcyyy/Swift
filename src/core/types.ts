export type TauriEvent<T> = {
    payload: T
};

export type BungieProfile = {
    membershipType: number;
    membershipId: string;
    bungieGlobalDisplayName: string;
    bungieGlobalDisplayNameCode: number;
    crossSaveOverride: number;
};

export type Profiles = {
    savedProfiles: Profile[],
    selectedProfile: Profile,
}

export type Profile = {
    accountPlatform: number;
    accountId: string;
};

export type ProfileInfo = {
    privacy: number;
    displayName: string;
    displayTag: number;
    characterIds: string[];
};

export type Preferences = {
    enableOverlay: boolean;
    displayDailyClears: boolean;
    displayTimer: boolean;
    displayWeeklyClears: boolean;
    displaySpotify: boolean;
    displayClearNotifications: boolean;
    displayMilliseconds: boolean;
    spotifyClientId: string;
    appGradient1: string;
    appGradient2: string;
    dailyColor: string;
    weeklyColor: string;
    spotifyAccentColor: string;
    overlayTextColor: string;
    overlayScale: number;
    overlayOpacity: number;
    overlayPosition: string;
    overlayOffsetX: number;
    overlayOffsetY: number;
};

export type PlayerDataStatus = {
    lastUpdate: PlayerData,
    error: string,
    // Milliseconds to add to the local clock to line it up with the Bungie
    // clock that currentActivity.startDate is measured against.
    clockOffsetMillis: number,
}

export type PlayerData = {
    currentActivity: CurrentActivity;
    activityHistory: CompletedActivity[];
    profileInfo: ProfileInfo;
};

export type CurrentActivity = {
    startDate: string;
    activityHash: number;
    activityInfo: ActivityInfo;
};

export type ActivityInfo = {
    name: string;
    activityModes: number[];
    backgroundImage: string;
};

export type CompletedActivity = {
    period: string;
    instanceId: string;
    completed: boolean;
    activityDuration: string;
    activityDurationSeconds: number;
    activityHash: number;
    modes: number[];
};
