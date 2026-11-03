# Daily rewards and account selection

Check-in cards start collapsed. Select the card title to expand or collapse its controls. Collapsing a card preserves the selected account and pending authorization; automatic check-in continues independently.

| Client          | Daily reward                                                | Account selection                                                                                                          |
| --------------- | ----------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| WorkBuddy China | Official campaign status and claim                          | Current desktop login plus independent browser-authorized accounts; each has its own automatic switch and daily result.    |
| Doubao Work     | Official client's campaign SDK                              | Accounts already signed in to the connected native client; check-in switches to the selected account and leaves it active. |
| Codex           | No daily check-in reward documented in the official service | Use the existing ChatGPT/Codex authentication manager and account-specific quota views.                                    |

WorkBuddy's native account picker includes account contexts; its currently saved desktop session is not a reusable pool of independently authorized refresh tokens. Use **Refresh accounts** after changing the desktop login to immediately reload the account list and local check-in results. Refreshing does not claim a reward. Use **Add account** on the check-in card for each separate browser grant. After authorization, choose that account to claim manually or enable its automatic switch. Starting or completing authorization never claims a reward or enables automatic check-in.

Doubao Work keeps its saved accounts within the official client. Sign in there and refresh VaryAI's list. **Switch and check in** uses the official account switcher, waits for the selected login, and then requests its reward. The account stays selected afterward. Automatic check-in processes enabled accounts in sequence and leaves the last processed account active. Completed daily results are reused without switching. A refused switch never falls back to claiming for the previous account.

Doubao Work's card shows the official quota reward, consecutive check-in days and total check-in days when the campaign provides them. Refresh queries the current account without claiming or switching; saved daily results remain specific to their account.

Automatic check-in requires VaryAI to stay open. WorkBuddy's independently authorized accounts can be processed separately; Doubao Work also needs its connected native client. Records are local to the device and separate from synchronized settings. See [WorkBuddy](workbuddy.md#daily-check-in) and [Doubao Work](doubao-work.md#daily-check-in-and-accounts) for credential handling and retry behavior.

Codex usage limits reset according to the subscription and usage window. Purchased credits and service promotions are different mechanisms. The [official pricing and usage documentation](https://learn.chatgpt.com/docs/pricing) does not document a daily reward to claim. VaryAI therefore does not offer a synthetic Codex check-in action.
