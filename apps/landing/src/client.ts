import './styles.css'
import './theme'
import { bindDemo } from './demo/play'
import { bindFlow } from './flow/animate'

export function bindMotion() {
  bindDemo()
  bindFlow()
}

bindMotion()
