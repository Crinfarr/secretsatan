import { Client, User } from "discord.js";
import fs from 'fs';

type Config = {
    token: string,
    users: string[]
}
const config = <Config>JSON.parse(fs.readFileSync('./config.secret.json').toString());

const bot = new Client({
    intents: [
        "DirectMessages",
    ]
});
const receivers = [...config.users];
const givers = [...config.users];
receivers.sort((_a, _b) => Math.round(Math.random() * 2) - 1);
givers.sort((_a, _b) => Math.round(Math.random() * 2) - 1);

bot.on('ready', async (client) => {
    const pairings: User[][] = []
    for (let giver of givers) {
        const gUser = await client.users.fetch(giver);
        let rUser: User;
        do {
            receivers.sort((_a, _b) => Math.round(Math.random()*2)-1);
        } while (receivers[receivers.length-1] == giver);
        rUser = await client.users.fetch(receivers.pop()!);
        pairings.push([gUser, rUser])
    }
    // console.log(pairings.map((val, _idx, _arr) => `${val[0].username} -> ${val[1].username}`));
    for (let [giver, receiver] of pairings) {
        await giver.send(`# THIS IS A TEST MESSAGE THESE ARENT THE REAL PAIRINGS\n\nYou are giving to <@${receiver.id}>`);
    }
    await bot.destroy();
});

bot.login(config.token);