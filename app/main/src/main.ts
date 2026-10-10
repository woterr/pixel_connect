// Pixel Connect — front end entry point.

import { createPinia } from 'pinia'
import { createApp } from 'vue'

import App from './App.vue'
import './assets/main.css'

createApp(App).use(createPinia()).mount('#app')
