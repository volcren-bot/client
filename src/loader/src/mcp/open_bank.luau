--!strict
local Bankmain = require("@volcren/bankmain")
local BankpinKeypad = require("@volcren/bankpin_keypad")
local Walker = require("@volcren/walker")

if Walker.here() == nil then
	print("The player isn't logged in.")
	return
end
if Bankmain.is_open() then
	print("The bank is already open.")
	return
end
local ok, result = pcall(function()
	return Bankmain.open()
end)
local at = Walker.here()
if not ok then
	print(`Couldn't open a bank: {result}`)
elseif result then
	print("The bank is open.")
elseif BankpinKeypad.is_open() then
	print(
		"The bank PIN keypad is still open: the loader has no PIN saved for this account, or the PIN was refused. Save it on the account's page under the loader's Accounts."
	)
else
	print("The bank didn't open.")
end
if at then
	print(`You are at {at.x}, {at.y}, plane {at.plane}.`)
end
